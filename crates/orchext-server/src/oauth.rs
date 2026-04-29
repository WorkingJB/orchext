//! OAuth 2.1 authorization-code grant with PKCE — agent token issuance.
//!
//! Three endpoints:
//! - `POST /v1/oauth/authorize` (session-authed) — a logged-in user
//!   approves an agent's request for a tenant-scoped token. Returns the
//!   one-time `code` for the client to redeem at `/token`. We don't
//!   render a consent UI here; the desktop / web client is the consent
//!   surface and posts JSON when the user clicks "approve."
//! - `POST /v1/oauth/token` (no session auth) — exchanges
//!   (`code`, `code_verifier`, `redirect_uri`) for an opaque `ocx_*`
//!   bearer token row in `mcp_tokens`. PKCE is mandatory and only S256
//!   is accepted (OAuth 2.1 §7.5.2 — `plain` is forbidden).
//! - `POST /v1/oauth/register` (session-authed for now) — RFC 7591
//!   client registration. Creates an `oauth_clients` row tagged
//!   `claude_connector` / `chatgpt_connector` / `copilot_connector` /
//!   `manual` and returns `client_id` + `client_secret` exactly once.
//!   Open / unauthenticated DCR (`origin = 'dynamic_registration'`)
//!   is deferred — none of the four target providers strictly need it
//!   when wizard-pre-registered clients are an option.
//!
//! D15 (opaque tokens) and D16 (rolled, no library) carry through
//! unchanged. The token returned here is a normal `mcp_tokens` row, so
//! `revoke_token`, `list_tokens`, scope evaluation, etc. all work
//! without further changes.

use crate::{
    cookies,
    error::ApiError,
    password,
    sessions::{AuthSource, SessionContext},
    tokens,
    AppState,
};
use axum::{
    extract::{OriginalUri, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Extension, Form, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use uuid::Uuid;

/// Authorization codes carry an `oac_` prefix to make them visually
/// distinct from `ocx_*` session/token secrets in logs and error reports.
const CODE_PREFIX: &str = "oac_";
const CODE_BYTES: usize = 32;
const PREFIX_LOOKUP_LEN: usize = CODE_PREFIX.len() + 8;
const CODE_TTL_SECS: i64 = 600; // 10 min — matches OAuth 2.1 guidance.
const VERIFIER_MIN_LEN: usize = 43; // RFC 7636 §4.1
const VERIFIER_MAX_LEN: usize = 128;

/// Public router. The `/authorize` route is session-authed by the caller
/// (`router(state)` mounts both routes; `/token` skips session auth
/// because the agent client doesn't have a user session — only the
/// auth code).
pub fn router() -> Router<AppState> {
    // Public OAuth routes:
    // - POST /token — agent client redeems an auth code; the code is
    //   the credential, no session required.
    // - GET /authorize — redirect-based authorization endpoint. The
    //   handler resolves the session itself (302 to /login if absent),
    //   so it can't sit behind session_auth which would 401 the
    //   browser.
    // - POST /authorize/decision — the consent screen's form target.
    //   Cookie-only auth + session-bound pending row stand in for CSRF
    //   middleware; see the handler doc.
    Router::new()
        .route("/token", post(token_handler))
        .route("/authorize", get(redirect_authorize_handler))
        .route("/authorize/decision", post(decision_handler))
}

pub fn authorize_router() -> Router<AppState> {
    Router::new()
        .route("/authorize", post(authorize_handler))
        .route("/register", post(register_handler))
}

// ---------- /authorize ----------

#[derive(Debug, Deserialize)]
struct AuthorizeRequest {
    /// Audience: the tenant the agent will operate against. Caller must
    /// be a member.
    tenant_id: Uuid,
    /// Free-form display name shown back in `mcp_tokens.label`. The
    /// client picks it; the user sees it in the token list.
    client_label: String,
    /// Where the issued auth code is delivered. Must be one of:
    /// - `http://127.0.0.1:<port>/...` or `http://localhost:<port>/...`
    ///   (loopback — desktop apps that bind a temporary listener)
    /// - `https://<host>/...` (web SPAs registered server-side later)
    /// Anything else is rejected — OAuth 2.1 §3.1.2 forbids non-HTTPS
    /// redirect URIs except for loopback.
    redirect_uri: String,
    /// Scope labels (visibility names — `public`, `work`, `personal`,
    /// or any custom label that round-trips through `Visibility`).
    scope: Vec<String>,
    /// `read` or `read_propose`. Defaults to `read`.
    #[serde(default)]
    mode: Option<String>,
    /// Token TTL in days. Defaults to 90; clamped to [1, 365] like
    /// other token issuance paths.
    #[serde(default)]
    ttl_days: Option<i64>,
    /// Per-token retrieval limit (documents). Defaults to 20.
    #[serde(default)]
    max_docs: Option<i32>,
    /// Per-token retrieval limit (bytes). Defaults to 65 536.
    #[serde(default)]
    max_bytes: Option<i64>,
    /// Base64url-encoded SHA-256 hash of the client's code verifier.
    /// Length is invariant: SHA-256 is 32 bytes → 43 chars unpadded.
    code_challenge: String,
    /// Must be `S256`. `plain` is rejected per OAuth 2.1.
    code_challenge_method: String,
}

#[derive(Debug, Serialize)]
struct AuthorizeResponse {
    /// The one-time auth code. Client posts this back to `/token`
    /// alongside the verifier. Single-use; expires in 10 minutes.
    code: String,
    /// Echoed for the client's convenience — same as the request's
    /// `redirect_uri`. Helps clients that round-trip through a browser
    /// and want to reconstruct the callback URL.
    redirect_uri: String,
    /// Seconds until `code` expires.
    expires_in: i64,
}

async fn authorize_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionContext>,
    Json(req): Json<AuthorizeRequest>,
) -> Result<(StatusCode, Json<AuthorizeResponse>), ApiError> {
    if req.code_challenge_method != "S256" {
        return Err(ApiError::InvalidArgument(
            "code_challenge_method must be S256".into(),
        ));
    }
    validate_code_challenge(&req.code_challenge)?;
    validate_redirect_uri(&req.redirect_uri)?;
    tokens::validate_label(&req.client_label)?;
    let scope = tokens::normalize_scope(req.scope)?;
    let mode = tokens::normalize_mode(req.mode.as_deref())?;
    let ttl_days = tokens::clamp_ttl_days(req.ttl_days);
    let max_docs = req.max_docs.unwrap_or(20).max(1);
    let max_bytes = req.max_bytes.unwrap_or(64 * 1024).max(1024);

    // Confirm the caller is a member of the requested tenant. Same
    // not-found-on-mismatch shape as `tenant_auth` so we don't leak
    // tenant existence to non-members.
    let member: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM memberships WHERE tenant_id = $1 AND account_id = $2",
    )
    .bind(req.tenant_id)
    .bind(session.account_id)
    .fetch_optional(&state.db)
    .await?;
    if member.is_none() {
        return Err(ApiError::NotFound);
    }

    let code = generate_code();
    let prefix = code[..PREFIX_LOOKUP_LEN].to_string();
    let hash = password::hash(&code).map_err(|e| ApiError::Internal(Box::new(e)))?;
    let expires_at = Utc::now() + Duration::seconds(CODE_TTL_SECS);

    sqlx::query(
        r#"
        INSERT INTO oauth_authorization_codes
            (code_prefix, code_hash, account_id, tenant_id, client_label,
             redirect_uri, scope, mode, max_docs, max_bytes, ttl_days,
             code_challenge, code_challenge_method, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        "#,
    )
    .bind(&prefix)
    .bind(&hash)
    .bind(session.account_id)
    .bind(req.tenant_id)
    .bind(&req.client_label)
    .bind(&req.redirect_uri)
    .bind(&scope)
    .bind(mode)
    .bind(max_docs)
    .bind(max_bytes)
    .bind(ttl_days as i32)
    .bind(&req.code_challenge)
    .bind(&req.code_challenge_method)
    .bind(expires_at)
    .execute(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(AuthorizeResponse {
            code,
            redirect_uri: req.redirect_uri,
            expires_in: CODE_TTL_SECS,
        }),
    ))
}

// ---------- /token ----------

#[derive(Debug, Deserialize)]
struct TokenRequest {
    /// OAuth 2.1 §4.1.3. Must be `authorization_code`.
    grant_type: String,
    code: String,
    code_verifier: String,
    /// Must match the `redirect_uri` posted to `/authorize`.
    redirect_uri: String,
    /// Confidential-client credentials (RFC 6749 §2.3.1) — only used
    /// for codes minted by the redirect flow. Either `client_id` +
    /// `client_secret` in the body or HTTP Basic auth in the header
    /// works; we accept both.
    #[serde(default)]
    client_id: Option<Uuid>,
    #[serde(default)]
    client_secret: Option<String>,
}

#[derive(Debug, Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str, // always "Bearer"
    expires_in: i64,          // seconds until token expires
    scope: String,            // space-separated, OAuth-idiomatic
    /// Tenant the token operates against — non-standard but useful for
    /// agent clients that want to skip a `/v1/tenants` round trip.
    tenant_id: Uuid,
    /// Internal mcp_tokens.id, useful for revocation by the issuing user.
    token_id: String,
}

#[derive(Debug, FromRow)]
struct CodeRow {
    code_hash: String,
    account_id: Uuid,
    tenant_id: Uuid,
    client_label: String,
    redirect_uri: String,
    scope: Vec<String>,
    mode: String,
    max_docs: i32,
    max_bytes: i64,
    ttl_days: i32,
    code_challenge: String,
    expires_at: DateTime<Utc>,
    used_at: Option<DateTime<Utc>>,
    /// Set when the code came from the redirect flow. Forces the
    /// client_secret_basic check at /token.
    client_id: Option<Uuid>,
}

async fn token_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<TokenResponse>, ApiError> {
    // RFC 6749 §3.2 says the token endpoint MUST accept
    // application/x-www-form-urlencoded. We also accept JSON so the
    // existing desktop oauth-client (which posts JSON) keeps working.
    let req: TokenRequest = parse_token_request(&headers, &body)?;

    if req.grant_type != "authorization_code" {
        return Err(ApiError::InvalidArgument(
            "grant_type must be authorization_code".into(),
        ));
    }
    if req.code.len() < PREFIX_LOOKUP_LEN || !req.code.starts_with(CODE_PREFIX) {
        return Err(ApiError::Unauthorized);
    }
    validate_verifier(&req.code_verifier)?;

    let prefix = &req.code[..PREFIX_LOOKUP_LEN];

    // Single-statement claim: select the row by prefix, verify the full
    // hash, then UPDATE used_at in a separate transaction-bound step
    // below. The UPDATE itself enforces single-use via `used_at IS NULL`.
    let row: Option<CodeRow> = sqlx::query_as(
        r#"
        SELECT code_hash, account_id, tenant_id, client_label, redirect_uri,
               scope, mode, max_docs, max_bytes, ttl_days, code_challenge,
               expires_at, used_at, client_id
        FROM oauth_authorization_codes
        WHERE code_prefix = $1
        "#,
    )
    .bind(prefix)
    .fetch_optional(&state.db)
    .await?;

    let Some(row) = row else {
        return Err(ApiError::Unauthorized);
    };
    if row.used_at.is_some() {
        return Err(ApiError::Unauthorized);
    }
    if row.expires_at <= Utc::now() {
        return Err(ApiError::Unauthorized);
    }

    let secret_ok = password::verify(&req.code, &row.code_hash)
        .map_err(|e| ApiError::Internal(Box::new(e)))?;
    if !secret_ok {
        return Err(ApiError::Unauthorized);
    }

    if !pkce_matches(&req.code_verifier, &row.code_challenge) {
        return Err(ApiError::Unauthorized);
    }

    if !redirect_uri_matches(&req.redirect_uri, &row.redirect_uri) {
        return Err(ApiError::Unauthorized);
    }

    // Confidential-client check. Codes minted by the redirect flow
    // (D47) carry a `client_id` and require Basic auth (or body
    // params) to redeem; desktop POST-authorize codes have client_id
    // = NULL and skip this branch entirely.
    if let Some(expected_client_id) = row.client_id {
        let presented = client_credentials(&headers, &req)?;
        if presented.client_id != expected_client_id {
            return Err(ApiError::Unauthorized);
        }
        verify_client_secret(&state, &presented).await?;
    }

    // Atomically mark the code used. Race a parallel redemption: only
    // one UPDATE will see `used_at IS NULL` and affect a row.
    let claimed = sqlx::query(
        "UPDATE oauth_authorization_codes
         SET used_at = now()
         WHERE code_prefix = $1 AND used_at IS NULL",
    )
    .bind(prefix)
    .execute(&state.db)
    .await?
    .rows_affected();
    if claimed == 0 {
        return Err(ApiError::Unauthorized);
    }

    // Issue the token via the same path as the admin tokens endpoint.
    let issued = tokens::issue_via_oauth(
        &state.db,
        tokens::OAuthIssueInput {
            tenant_id: row.tenant_id,
            issued_by: row.account_id,
            label: row.client_label,
            scope: row.scope.clone(),
            mode: row.mode.clone(),
            max_docs: row.max_docs,
            max_bytes: row.max_bytes,
            ttl_days: row.ttl_days as i64,
            oauth_client_id: row.client_id,
        },
    )
    .await?;

    // Best-effort `last_used_at` touch on the registered client. Drives
    // the wizard's "Connected ✓" flip in 3f.2; failures are non-fatal.
    if let Some(client_id) = row.client_id {
        if let Err(e) = sqlx::query(
            "UPDATE oauth_clients SET last_used_at = now() WHERE client_id = $1",
        )
        .bind(client_id)
        .execute(&state.db)
        .await
        {
            tracing::debug!(client_id = %client_id, err = %e, "oauth_clients last_used_at touch failed");
        }
    }

    Ok(Json(TokenResponse {
        access_token: issued.secret,
        token_type: "Bearer",
        expires_in: (issued.expires_at - Utc::now()).num_seconds().max(0),
        scope: row.scope.join(" "),
        tenant_id: row.tenant_id,
        token_id: issued.id,
    }))
}

// ---------- /register ----------
//
// RFC 7591 dynamic client registration, scoped to a session-authed
// caller. The wizard (3f.2) calls this once per "Connect to …" press
// to mint a client bound to the active org+tenant; advanced users
// can also call it directly to pre-register a long-lived `manual`
// client without going through the connector tile.
//
// Client secrets follow the same shape as `mcp_tokens` rows: opaque,
// prefixed (`ocs_`), Argon2id-hashed at rest, looked up by an 8-byte
// prefix. Single-use: the secret is returned exactly once in the
// response and never recoverable after.

const CLIENT_SECRET_PREFIX: &str = "ocs_";
const CLIENT_SECRET_BYTES: usize = 32;
const CLIENT_SECRET_PREFIX_LOOKUP_LEN: usize = CLIENT_SECRET_PREFIX.len() + 8;

/// Origins valid for session-authed registration. `dynamic_registration`
/// is reserved for the deferred open-DCR endpoint; rejecting it here
/// keeps a session-authed caller from forging that label.
const VALID_REGISTER_ORIGINS: &[&str] = &[
    "claude_connector",
    "chatgpt_connector",
    "copilot_connector",
    "manual",
];

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    /// Tenant the client (and any tokens it later mints) will operate
    /// against. Caller must be a member.
    tenant_id: Uuid,
    /// Free-form, surfaces in the consent screen + Tokens pane.
    client_name: String,
    /// RFC 7591 §2: array of allowed redirect URIs. At least one;
    /// each must satisfy the same HTTPS-or-loopback rule as
    /// `/authorize`.
    redirect_uris: Vec<String>,
    /// Wizard tag. Defaults to `manual` when absent so an advanced
    /// caller can use this endpoint without picking a connector
    /// flavor.
    #[serde(default)]
    origin: Option<String>,
    /// Visibility labels the connector defaults to (consent screen
    /// pre-fills these). Subset rules apply at /authorize time.
    #[serde(default)]
    default_scope: Option<Vec<String>>,
    /// `read` (default) or `read_propose`.
    #[serde(default)]
    default_mode: Option<String>,
}

#[derive(Debug, Serialize)]
struct RegisterResponse {
    client_id: Uuid,
    /// Returned exactly once. Persist immediately — there's no
    /// recovery path after this response.
    client_secret: String,
    /// Unix-seconds. RFC 7591 §3.2.1.
    client_id_issued_at: i64,
    /// `0` per RFC 7591 means the secret has no time-based expiry —
    /// revocation (3f.2 wizard *Disconnect* button) is the kill
    /// switch.
    client_secret_expires_at: i64,
    client_name: String,
    redirect_uris: Vec<String>,
    /// Echoed RFC-7591 fields so callers can confirm what was stored.
    grant_types: Vec<&'static str>,
    response_types: Vec<&'static str>,
    token_endpoint_auth_method: &'static str,
    /// Our extensions to the response. Useful for the wizard so it
    /// doesn't have to re-fetch.
    origin: String,
    default_scope: Vec<String>,
    default_mode: String,
}

async fn register_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionContext>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), ApiError> {
    if req.client_name.trim().is_empty() || req.client_name.len() > 200 {
        return Err(ApiError::InvalidArgument(
            "client_name must be 1..=200 chars".into(),
        ));
    }
    if req.redirect_uris.is_empty() {
        return Err(ApiError::InvalidArgument(
            "redirect_uris must contain at least one URI".into(),
        ));
    }
    for uri in &req.redirect_uris {
        validate_redirect_uri(uri)?;
    }

    let origin = req.origin.unwrap_or_else(|| "manual".to_string());
    if !VALID_REGISTER_ORIGINS.contains(&origin.as_str()) {
        return Err(ApiError::InvalidArgument(format!(
            "origin must be one of {VALID_REGISTER_ORIGINS:?}"
        )));
    }

    // An empty default scope is fine — consent screen falls back to
    // the user's full read set as the picker default. A non-empty
    // scope still has to validate.
    let scope = match req.default_scope {
        Some(s) if !s.is_empty() => tokens::normalize_scope(s)?,
        _ => Vec::new(),
    };
    let mode = tokens::normalize_mode(req.default_mode.as_deref())?.to_string();

    // Tenant membership check, same shape (and same "no info on
    // mismatch") as authorize_handler.
    let member: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM memberships WHERE tenant_id = $1 AND account_id = $2",
    )
    .bind(req.tenant_id)
    .bind(session.account_id)
    .fetch_optional(&state.db)
    .await?;
    if member.is_none() {
        return Err(ApiError::NotFound);
    }

    let secret = generate_client_secret();
    let secret_prefix = secret[..CLIENT_SECRET_PREFIX_LOOKUP_LEN].to_string();
    let secret_hash =
        password::hash(&secret).map_err(|e| ApiError::Internal(Box::new(e)))?;

    let row: (Uuid, DateTime<Utc>) = sqlx::query_as(
        r#"
        INSERT INTO oauth_clients
            (client_secret_prefix, client_secret_hash, client_name,
             redirect_uris, origin, tenant_id, account_id,
             default_scope, default_mode)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING client_id, created_at
        "#,
    )
    .bind(&secret_prefix)
    .bind(&secret_hash)
    .bind(&req.client_name)
    .bind(&req.redirect_uris)
    .bind(&origin)
    .bind(req.tenant_id)
    .bind(session.account_id)
    .bind(&scope)
    .bind(&mode)
    .fetch_one(&state.db)
    .await?;

    let (client_id, created_at) = row;
    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            client_id,
            client_secret: secret,
            client_id_issued_at: created_at.timestamp(),
            client_secret_expires_at: 0,
            client_name: req.client_name,
            redirect_uris: req.redirect_uris,
            grant_types: vec!["authorization_code"],
            response_types: vec!["code"],
            token_endpoint_auth_method: "client_secret_basic",
            origin,
            default_scope: scope,
            default_mode: mode,
        }),
    ))
}

fn generate_client_secret() -> String {
    let mut bytes = [0u8; CLIENT_SECRET_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{CLIENT_SECRET_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

// ---------- GET /authorize (redirect flow) + POST /authorize/decision ----------
//
// The provider's user-agent (Claude/ChatGPT/Copilot's browser tab)
// hits GET /authorize with the standard OAuth 2.1 code-flow query
// params. We render an HTML consent screen and wait for the user to
// click Approve or Deny on the form, which posts to
// /authorize/decision. On approve we mint a normal
// oauth_authorization_codes row and 302 back to the provider's
// redirect_uri carrying ?code=...&state=...; on deny we 302 with
// ?error=access_denied.
//
// Two cuts kept this slice small:
// - No standalone scope picker on the consent screen — we surface the
//   client's `default_scope` in copy and let the user pick "Allow" or
//   "Deny" against that bundle. Scope narrowing is a wizard feature
//   (3f.2), not a consent-screen feature.
// - No registration_access_token / dynamic update — the wizard's
//   "Disconnect" button revokes via `oauth_clients.revoked_at` and
//   that's the kill switch.

const PENDING_TTL_SECS: i64 = 600; // 10 min — same as code TTL.
const TOKEN_DEFAULT_MAX_DOCS: i32 = 20;
const TOKEN_DEFAULT_MAX_BYTES: i64 = 64 * 1024;
const TOKEN_DEFAULT_TTL_DAYS: i32 = 90;

#[derive(Debug, Deserialize)]
struct RedirectAuthorizeQuery {
    response_type: String,
    /// Registered client identifier — UUID parse failure is a 400.
    client_id: Uuid,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String,
    /// OAuth `state`: opaque to us; echoed verbatim on the redirect
    /// response. RFC 6749 RECOMMENDS but doesn't require.
    #[serde(default)]
    state: Option<String>,
    /// Space-separated visibility labels per RFC 6749 §3.3. Optional —
    /// unset falls back to the client's `default_scope`.
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, FromRow)]
struct ClientRow {
    client_id: Uuid,
    client_name: String,
    redirect_uris: Vec<String>,
    tenant_id: Uuid,
    default_scope: Vec<String>,
    default_mode: String,
    revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
struct PendingRow {
    request_id: Uuid,
    client_id: Uuid,
    account_id: Uuid,
    redirect_uri: String,
    state: Option<String>,
    scope: Vec<String>,
    mode: String,
    code_challenge: String,
    code_challenge_method: String,
    expires_at: DateTime<Utc>,
}

async fn redirect_authorize_handler(
    State(state): State<AppState>,
    Query(q): Query<RedirectAuthorizeQuery>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // Reject obviously malformed requests with a 400 page rather than
    // redirecting; OAuth 2.1 §4.1.2.1 forbids redirecting to a URI we
    // can't trust, and these checks are what tell us if we trust it.
    if q.response_type != "code" {
        return Ok(error_html(
            StatusCode::BAD_REQUEST,
            "response_type must be `code`",
        ));
    }

    let client = match lookup_client(&state, q.client_id).await? {
        Some(c) if c.revoked_at.is_none() => c,
        _ => {
            return Ok(error_html(
                StatusCode::BAD_REQUEST,
                "Unknown or revoked client_id.",
            ))
        }
    };

    if !client
        .redirect_uris
        .iter()
        .any(|u| u == &q.redirect_uri)
    {
        return Ok(error_html(
            StatusCode::BAD_REQUEST,
            "redirect_uri is not registered for this client.",
        ));
    }

    // Past this point we have a verified redirect_uri, so subsequent
    // protocol-level errors can be reported via a 302 carrying
    // `?error=…&state=…` per OAuth 2.1 §4.1.2.1.
    if q.code_challenge_method != "S256" {
        return Ok(redirect_with_error(
            &q.redirect_uri,
            "invalid_request",
            Some("code_challenge_method must be S256"),
            q.state.as_deref(),
        ));
    }
    if validate_code_challenge(&q.code_challenge).is_err() {
        return Ok(redirect_with_error(
            &q.redirect_uri,
            "invalid_request",
            Some("code_challenge is malformed"),
            q.state.as_deref(),
        ));
    }

    // Resolve session via cookie. If absent or invalid, 302 to the
    // SPA login with `next` set to this very URL so the user lands
    // back on the consent screen post-login.
    let cookies_map = cookies::parse(&headers);
    let session_secret = cookies_map.get(cookies::SESSION_COOKIE);
    let Some(secret) = session_secret else {
        return Ok(redirect_to_login(&uri));
    };
    let session_ctx = match state
        .sessions
        .authenticate(secret, AuthSource::Cookie)
        .await
    {
        Ok(c) => c,
        Err(_) => return Ok(redirect_to_login(&uri)),
    };

    // Membership check: a connector's auth flow is meaningful only for
    // a user who actually belongs to the tenant the client binds.
    let member: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM memberships WHERE tenant_id = $1 AND account_id = $2",
    )
    .bind(client.tenant_id)
    .bind(session_ctx.account_id)
    .fetch_optional(&state.db)
    .await?;
    if member.is_none() {
        return Ok(error_html(
            StatusCode::FORBIDDEN,
            "Your account is not a member of the organization this connector is bound to.",
        ));
    }

    // Resolve effective scope. Spec-form `scope` is space-separated.
    let scope: Vec<String> = match q.scope.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            s.split_whitespace().map(str::to_string).collect()
        }
        _ if !client.default_scope.is_empty() => client.default_scope.clone(),
        _ => {
            // No scope on the request and no default on the client
            // means the consent screen would be approving "nothing,"
            // which should be a hard error rather than a noop token.
            return Ok(redirect_with_error(
                &q.redirect_uri,
                "invalid_scope",
                Some("scope must be specified or default_scope must be set on the client"),
                q.state.as_deref(),
            ));
        }
    };

    // Park the pending request. The decision handler reads it back and
    // verifies the same account_id is still on the session.
    let row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO oauth_pending_authorizations
            (client_id, account_id, redirect_uri, state, scope, mode,
             code_challenge, code_challenge_method, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING request_id
        "#,
    )
    .bind(client.client_id)
    .bind(session_ctx.account_id)
    .bind(&q.redirect_uri)
    .bind(q.state.as_deref())
    .bind(&scope)
    .bind(&client.default_mode)
    .bind(&q.code_challenge)
    .bind(&q.code_challenge_method)
    .bind(Utc::now() + Duration::seconds(PENDING_TTL_SECS))
    .fetch_one(&state.db)
    .await?;
    let request_id = row.0;

    Ok(Html(consent_html(
        &client.client_name,
        &scope,
        &client.default_mode,
        request_id,
    ))
    .into_response())
}

#[derive(Debug, Deserialize)]
struct DecisionForm {
    request_id: Uuid,
    /// `approve` or `deny`. Anything else 400s — the only two buttons
    /// on the consent form post these literal values.
    action: String,
}

async fn decision_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<DecisionForm>,
) -> Result<Response, ApiError> {
    // Cookie-only session check; no CSRF middleware sits in front of
    // this route. The pending row is the CSRF defense: it's bound to
    // the account_id that started the consent screen, and request_id
    // is only ever set by this server in response to a same-origin
    // GET on the same browser.
    let cookies_map = cookies::parse(&headers);
    let secret = cookies_map
        .get(cookies::SESSION_COOKIE)
        .ok_or(ApiError::Unauthorized)?;
    let session_ctx = state
        .sessions
        .authenticate(secret, AuthSource::Cookie)
        .await?;

    let pending: Option<PendingRow> = sqlx::query_as(
        r#"
        SELECT request_id, client_id, account_id, redirect_uri, state, scope,
               mode, code_challenge, code_challenge_method, expires_at
        FROM oauth_pending_authorizations
        WHERE request_id = $1
        "#,
    )
    .bind(form.request_id)
    .fetch_optional(&state.db)
    .await?;
    let Some(pending) = pending else {
        return Err(ApiError::NotFound);
    };
    if pending.account_id != session_ctx.account_id {
        return Err(ApiError::Unauthorized);
    }
    if pending.expires_at <= Utc::now() {
        sqlx::query("DELETE FROM oauth_pending_authorizations WHERE request_id = $1")
            .bind(pending.request_id)
            .execute(&state.db)
            .await
            .ok();
        return Ok(error_html(
            StatusCode::BAD_REQUEST,
            "This consent request expired. Restart the connection from the provider.",
        ));
    }

    // Always clear the pending row on this code path. If the approve
    // INSERT fails downstream the client just retries from scratch.
    let _ = sqlx::query("DELETE FROM oauth_pending_authorizations WHERE request_id = $1")
        .bind(pending.request_id)
        .execute(&state.db)
        .await;

    if form.action == "deny" {
        return Ok(redirect_with_error(
            &pending.redirect_uri,
            "access_denied",
            None,
            pending.state.as_deref(),
        ));
    }
    if form.action != "approve" {
        return Err(ApiError::InvalidArgument(
            "action must be 'approve' or 'deny'".into(),
        ));
    }

    // Re-fetch the client to pull tenant_id + label. The pending row
    // intentionally doesn't denormalize these — they live on
    // oauth_clients and a revoke during the consent window should
    // invalidate the in-flight code.
    let client = match lookup_client(&state, pending.client_id).await? {
        Some(c) if c.revoked_at.is_none() => c,
        _ => {
            return Ok(error_html(
                StatusCode::BAD_REQUEST,
                "Connector was revoked while the consent screen was open.",
            ))
        }
    };

    // Mint a real authorization code on the existing schema. Same
    // shape POST /authorize would have produced — the only differences
    // are the bound client_id and the scope/mode coming from the
    // pending row instead of the request.
    let code = generate_code();
    let prefix = code[..PREFIX_LOOKUP_LEN].to_string();
    let hash = password::hash(&code).map_err(|e| ApiError::Internal(Box::new(e)))?;
    let expires_at = Utc::now() + Duration::seconds(CODE_TTL_SECS);

    sqlx::query(
        r#"
        INSERT INTO oauth_authorization_codes
            (code_prefix, code_hash, account_id, tenant_id, client_label,
             redirect_uri, scope, mode, max_docs, max_bytes, ttl_days,
             code_challenge, code_challenge_method, expires_at, client_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
        "#,
    )
    .bind(&prefix)
    .bind(&hash)
    .bind(session_ctx.account_id)
    .bind(client.tenant_id)
    .bind(&client.client_name)
    .bind(&pending.redirect_uri)
    .bind(&pending.scope)
    .bind(&pending.mode)
    .bind(TOKEN_DEFAULT_MAX_DOCS)
    .bind(TOKEN_DEFAULT_MAX_BYTES)
    .bind(TOKEN_DEFAULT_TTL_DAYS)
    .bind(&pending.code_challenge)
    .bind(&pending.code_challenge_method)
    .bind(expires_at)
    .bind(client.client_id)
    .execute(&state.db)
    .await?;

    Ok(redirect_with_code(
        &pending.redirect_uri,
        &code,
        pending.state.as_deref(),
    ))
}

async fn lookup_client(
    state: &AppState,
    client_id: Uuid,
) -> Result<Option<ClientRow>, ApiError> {
    let row: Option<ClientRow> = sqlx::query_as(
        r#"
        SELECT client_id, client_name, redirect_uris, tenant_id,
               default_scope, default_mode, revoked_at
        FROM oauth_clients
        WHERE client_id = $1
        "#,
    )
    .bind(client_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(row)
}

/// 302 to the SPA login with `?next=<this URL>` so the user lands
/// back on the consent screen after authenticating.
fn redirect_to_login(original: &axum::http::Uri) -> Response {
    let next = original
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let mut url = url::Url::parse("http://placeholder/login")
        .expect("static URL parses");
    url.query_pairs_mut().append_pair("next", next);
    // Drop scheme/host — Redirect::to wants a path-only string when
    // the target is same-origin.
    let location = format!(
        "/login{}",
        url.query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default()
    );
    Redirect::to(&location).into_response()
}

fn redirect_with_code(redirect_uri: &str, code: &str, state: Option<&str>) -> Response {
    let mut url = match url::Url::parse(redirect_uri) {
        Ok(u) => u,
        Err(_) => {
            return error_html(StatusCode::BAD_REQUEST, "redirect_uri is not a valid URL")
        }
    };
    url.query_pairs_mut().append_pair("code", code);
    if let Some(s) = state {
        url.query_pairs_mut().append_pair("state", s);
    }
    Redirect::to(url.as_str()).into_response()
}

fn redirect_with_error(
    redirect_uri: &str,
    error: &str,
    description: Option<&str>,
    state: Option<&str>,
) -> Response {
    let mut url = match url::Url::parse(redirect_uri) {
        Ok(u) => u,
        Err(_) => {
            return error_html(StatusCode::BAD_REQUEST, "redirect_uri is not a valid URL")
        }
    };
    url.query_pairs_mut().append_pair("error", error);
    if let Some(d) = description {
        url.query_pairs_mut().append_pair("error_description", d);
    }
    if let Some(s) = state {
        url.query_pairs_mut().append_pair("state", s);
    }
    Redirect::to(url.as_str()).into_response()
}

/// Hand-written HTML — minimum-viable consent screen. No styling
/// framework, no JS bundle: a server-rendered page is the simpler
/// surface here, since the SPA isn't on this URL.
fn consent_html(client_name: &str, scope: &[String], mode: &str, request_id: Uuid) -> String {
    let scope_list = scope
        .iter()
        .map(|s| format!("<li>{}</li>", html_escape(s)))
        .collect::<String>();
    let mode_label = if mode == "read_propose" {
        "read + propose changes"
    } else {
        "read"
    };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Allow access — Orchext</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
body {{ font-family: -apple-system, system-ui, sans-serif; max-width: 480px; margin: 4rem auto; padding: 0 1rem; color: #111; }}
h1 {{ font-size: 1.25rem; }}
ul {{ padding-left: 1.25rem; }}
.actions {{ display: flex; gap: 0.75rem; margin-top: 1.5rem; }}
button {{ font: inherit; padding: 0.5rem 1rem; border-radius: 6px; border: 1px solid #ccc; background: #fff; cursor: pointer; }}
button.primary {{ background: #1a56db; border-color: #1a56db; color: #fff; }}
.muted {{ color: #555; font-size: 0.9rem; }}
</style>
</head>
<body>
<h1>{client_name_escaped} wants to read your Orchext context</h1>
<p>Granting this connector lets <strong>{client_name_escaped}</strong> retrieve documents at the visibility levels listed below ({mode_label}).</p>
<ul>
{scope_list}</ul>
<form method="post" action="/v1/oauth/authorize/decision">
<input type="hidden" name="request_id" value="{request_id}">
<div class="actions">
<button type="submit" name="action" value="approve" class="primary">Allow</button>
<button type="submit" name="action" value="deny">Deny</button>
</div>
</form>
<p class="muted">You can revoke access any time in Settings → Tokens.</p>
</body>
</html>"#,
        client_name_escaped = html_escape(client_name),
        mode_label = mode_label,
        scope_list = scope_list,
        request_id = request_id,
    )
}

fn error_html(status: StatusCode, message: &str) -> Response {
    let body = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Error — Orchext</title></head>
<body style="font-family: -apple-system, system-ui, sans-serif; max-width: 480px; margin: 4rem auto;">
<h1>Cannot complete this connection</h1>
<p>{}</p>
</body></html>"#,
        html_escape(message)
    );
    (status, Html(body)).into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// ---------- token-endpoint helpers ----------

/// Decode a TokenRequest from either JSON or form body, picked by
/// content-type. RFC 6749 §3.2 mandates form support; the existing
/// desktop client posts JSON, so we accept both.
fn parse_token_request(
    headers: &HeaderMap,
    body: &axum::body::Bytes,
) -> Result<TokenRequest, ApiError> {
    let ct = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if ct.starts_with("application/json") {
        serde_json::from_slice::<TokenRequest>(body)
            .map_err(|e| ApiError::InvalidArgument(format!("invalid JSON: {e}")))
    } else if ct.starts_with("application/x-www-form-urlencoded") {
        serde_urlencoded::from_bytes::<TokenRequest>(body)
            .map_err(|e| ApiError::InvalidArgument(format!("invalid form body: {e}")))
    } else if ct.is_empty() {
        // Default to form per the OAuth spec when callers omit the
        // header — saves a 400 on otherwise-correct curl requests.
        serde_urlencoded::from_bytes::<TokenRequest>(body)
            .map_err(|e| ApiError::InvalidArgument(format!("invalid form body: {e}")))
    } else {
        Err(ApiError::InvalidArgument(format!(
            "unsupported content-type {ct:?}; expected application/json or \
             application/x-www-form-urlencoded"
        )))
    }
}

struct ClientCredentials {
    client_id: Uuid,
    client_secret: String,
}

/// Extract client credentials from either HTTP Basic auth or the
/// token request body. RFC 6749 §2.3.1 lists Basic as the primary
/// method; the body-param form is permitted but discouraged. The
/// confidential-client codes minted by the redirect flow can present
/// either.
fn client_credentials(
    headers: &HeaderMap,
    req: &TokenRequest,
) -> Result<ClientCredentials, ApiError> {
    if let Some(authz) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(b64) = authz.strip_prefix("Basic ").or_else(|| authz.strip_prefix("basic ")) {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|_| ApiError::Unauthorized)?;
            let s = std::str::from_utf8(&bytes).map_err(|_| ApiError::Unauthorized)?;
            let (id_str, secret) = s.split_once(':').ok_or(ApiError::Unauthorized)?;
            let id = Uuid::parse_str(id_str).map_err(|_| ApiError::Unauthorized)?;
            return Ok(ClientCredentials {
                client_id: id,
                client_secret: secret.to_string(),
            });
        }
    }
    if let (Some(id), Some(secret)) = (req.client_id, req.client_secret.clone()) {
        return Ok(ClientCredentials {
            client_id: id,
            client_secret: secret,
        });
    }
    Err(ApiError::Unauthorized)
}

async fn verify_client_secret(
    state: &AppState,
    creds: &ClientCredentials,
) -> Result<(), ApiError> {
    #[derive(FromRow)]
    struct Row {
        client_secret_hash: String,
        revoked_at: Option<DateTime<Utc>>,
    }
    let row: Option<Row> = sqlx::query_as(
        "SELECT client_secret_hash, revoked_at FROM oauth_clients WHERE client_id = $1",
    )
    .bind(creds.client_id)
    .fetch_optional(&state.db)
    .await?;
    let Some(row) = row else {
        return Err(ApiError::Unauthorized);
    };
    if row.revoked_at.is_some() {
        return Err(ApiError::Unauthorized);
    }
    let ok = password::verify(&creds.client_secret, &row.client_secret_hash)
        .map_err(|e| ApiError::Internal(Box::new(e)))?;
    if !ok {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}

// ---------- helpers ----------

fn generate_code() -> String {
    let mut bytes = [0u8; CODE_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{CODE_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

fn validate_code_challenge(s: &str) -> Result<(), ApiError> {
    // SHA-256 → 32 bytes → base64url-no-pad → exactly 43 chars.
    if s.len() != 43 {
        return Err(ApiError::InvalidArgument(
            "code_challenge must be base64url(SHA256(verifier)) — 43 chars".into(),
        ));
    }
    if !s.chars().all(is_base64url_char) {
        return Err(ApiError::InvalidArgument(
            "code_challenge contains non-base64url chars".into(),
        ));
    }
    Ok(())
}

fn validate_verifier(s: &str) -> Result<(), ApiError> {
    if s.len() < VERIFIER_MIN_LEN || s.len() > VERIFIER_MAX_LEN {
        return Err(ApiError::InvalidArgument(format!(
            "code_verifier length must be {VERIFIER_MIN_LEN}..={VERIFIER_MAX_LEN}",
        )));
    }
    // RFC 7636 §4.1: ALPHA / DIGIT / - . _ ~
    let ok = s.bytes().all(|b| {
        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
    });
    if !ok {
        return Err(ApiError::InvalidArgument(
            "code_verifier contains disallowed characters".into(),
        ));
    }
    Ok(())
}

fn is_base64url_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// SHA-256 the verifier and base64url-encode the result. Compare in
/// constant time against the stored challenge.
fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    let mut h = Sha256::new();
    h.update(verifier.as_bytes());
    let computed = URL_SAFE_NO_PAD.encode(h.finalize());
    use subtle::ConstantTimeEq;
    computed.as_bytes().ct_eq(challenge.as_bytes()).into()
}

/// Exact-match the redirect URI presented at /token against the one
/// stored at /authorize. OAuth 2.1 §3.1.2.3: byte-exact match required.
fn redirect_uri_matches(presented: &str, stored: &str) -> bool {
    use subtle::ConstantTimeEq;
    presented.as_bytes().ct_eq(stored.as_bytes()).into()
}

/// OAuth 2.1 §3.1.2.1 / §10.3.3: redirect URIs must be HTTPS, with
/// loopback HTTP allowed for native apps that bind a local listener.
fn validate_redirect_uri(uri: &str) -> Result<(), ApiError> {
    let lower = uri.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    // Loopback variants (RFC 8252 §7.3) — match scheme + host + ':'.
    // We don't parse the URL fully; just check the prefix because any
    // other scheme/host combination is rejected.
    if lower.starts_with("http://127.0.0.1:")
        || lower.starts_with("http://127.0.0.1/")
        || lower == "http://127.0.0.1"
        || lower.starts_with("http://localhost:")
        || lower.starts_with("http://localhost/")
        || lower == "http://localhost"
        || lower.starts_with("http://[::1]:")
        || lower.starts_with("http://[::1]/")
        || lower == "http://[::1]"
    {
        return Ok(());
    }
    Err(ApiError::InvalidArgument(
        "redirect_uri must be https or a loopback http URL".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_round_trip() {
        let verifier = "abcDEF123-._~xyzABCDEFGHIJKLMNOPQRSTUVWXYZ012345";
        let mut h = Sha256::new();
        h.update(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(h.finalize());
        assert!(pkce_matches(verifier, &challenge));
    }

    #[test]
    fn pkce_wrong_verifier_rejected() {
        let verifier = "abcDEF123-._~xyzABCDEFGHIJKLMNOPQRSTUVWXYZ012345";
        let other = "xyzDEF123-._~abcABCDEFGHIJKLMNOPQRSTUVWXYZ012345";
        let mut h = Sha256::new();
        h.update(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(h.finalize());
        assert!(!pkce_matches(other, &challenge));
    }

    #[test]
    fn validate_redirect_uri_https() {
        assert!(validate_redirect_uri("https://example.com/cb").is_ok());
        assert!(validate_redirect_uri("HTTPS://EXAMPLE.COM/cb").is_ok());
    }

    #[test]
    fn validate_redirect_uri_loopback() {
        assert!(validate_redirect_uri("http://127.0.0.1:5555/cb").is_ok());
        assert!(validate_redirect_uri("http://localhost:8080/cb").is_ok());
        assert!(validate_redirect_uri("http://[::1]:9000/cb").is_ok());
    }

    #[test]
    fn validate_redirect_uri_rejects_non_loopback_http() {
        assert!(validate_redirect_uri("http://example.com/cb").is_err());
        assert!(validate_redirect_uri("http://192.168.1.1/cb").is_err());
        assert!(validate_redirect_uri("ftp://example.com/cb").is_err());
        assert!(validate_redirect_uri("javascript:alert(1)").is_err());
    }

    #[test]
    fn challenge_length_pinned() {
        let valid = "a".repeat(43);
        assert!(validate_code_challenge(&valid).is_ok());
        let too_short = "a".repeat(42);
        assert!(validate_code_challenge(&too_short).is_err());
        let too_long = "a".repeat(44);
        assert!(validate_code_challenge(&too_long).is_err());
    }

    #[test]
    fn verifier_length_bounds() {
        let min_ok = "a".repeat(VERIFIER_MIN_LEN);
        assert!(validate_verifier(&min_ok).is_ok());
        let max_ok = "a".repeat(VERIFIER_MAX_LEN);
        assert!(validate_verifier(&max_ok).is_ok());
        let too_short = "a".repeat(VERIFIER_MIN_LEN - 1);
        assert!(validate_verifier(&too_short).is_err());
        let too_long = "a".repeat(VERIFIER_MAX_LEN + 1);
        assert!(validate_verifier(&too_long).is_err());
    }

    #[test]
    fn verifier_rejects_disallowed_chars() {
        let bad = "abc/DEF=GHI+JKL".repeat(4);
        assert!(validate_verifier(&bad).is_err());
    }
}
