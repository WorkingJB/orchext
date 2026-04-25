//! Crypto control-plane endpoints.
//!
//! - `GET  /v1/t/:tid/vault/crypto`        — fetch salt + wrapped content key
//!   (null-ish response if the tenant hasn't seeded crypto yet).
//! - `POST /v1/t/:tid/vault/init-crypto`   — first-time seed; forbidden
//!   if already seeded. Client provides the salt + wrapped content key
//!   derived from the user's chosen passphrase, plus a key-check blob
//!   that lets the server verify subsequent published keys without
//!   ever holding the passphrase.
//! - `POST /v1/t/:tid/session-key`         — publish or refresh the live
//!   content key in the server's in-memory store. Verified against the
//!   stored key-check blob; mismatched keys are rejected.
//! - `DELETE /v1/t/:tid/session-key`       — drop the live key for the
//!   calling session.
//!
//! No endpoint ever returns the raw content key — only its wrapped
//! form. The raw key crosses the wire only on the inbound publish
//! request (under TLS, bearer-authed).

use crate::{
    error::ApiError, session_keys::DEFAULT_TTL, sessions::SessionContext,
    tenants::TenantContext, AppState,
};
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use orchext_crypto::{verify_key_check, ContentKey, SealedBlob};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/vault/crypto", get(get_crypto))
        .route("/vault/init-crypto", post(init_crypto))
        .route(
            "/session-key",
            post(publish_session_key).delete(revoke_session_key),
        )
}

// ---------- GET /vault/crypto ----------

#[derive(Debug, Serialize)]
struct CryptoStateResponse {
    /// True when this tenant has crypto seeded. False means the
    /// tenant is still operating in plaintext mode (legacy / fresh).
    seeded: bool,
    /// Base64url KDF salt, null when `seeded = false`.
    kdf_salt: Option<String>,
    /// Wrapped content key (base64url sealed blob), null when
    /// `seeded = false`.
    wrapped_content_key: Option<String>,
    key_version: Option<i32>,
    /// True if the calling session currently has a live content key
    /// published. Informational — no secret leaks either way.
    unlocked: bool,
}

async fn get_crypto(
    State(state): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    Extension(sc): Extension<SessionContext>,
) -> Result<Json<CryptoStateResponse>, ApiError> {
    let row: Option<(Option<String>, Option<String>, Option<i32>)> = sqlx::query_as(
        "SELECT kdf_salt, wrapped_content_key, key_version FROM tenants WHERE id = $1",
    )
    .bind(tc.tenant_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((salt, wrapped, version)) = row else {
        return Err(ApiError::NotFound);
    };
    let seeded = salt.is_some() && wrapped.is_some();
    let unlocked = state.session_keys.is_unlocked(tc.tenant_id, sc.session_id);
    Ok(Json(CryptoStateResponse {
        seeded,
        kdf_salt: salt,
        wrapped_content_key: wrapped,
        key_version: version,
        unlocked,
    }))
}

// ---------- POST /vault/init-crypto ----------

#[derive(Debug, Deserialize)]
struct InitCryptoRequest {
    kdf_salt: String,
    wrapped_content_key: String,
    /// Domain-separated marker plaintext sealed with the content key.
    /// Lets `publish_session_key` verify a submitted key matches the
    /// wrapped one without the server ever holding the passphrase.
    key_check: String,
}

#[derive(Debug, Serialize)]
struct InitCryptoResponse {
    key_version: i32,
}

async fn init_crypto(
    State(state): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    Json(req): Json<InitCryptoRequest>,
) -> Result<(StatusCode, Json<InitCryptoResponse>), ApiError> {
    // Admin-only: only an owner or admin can seed tenant crypto,
    // since the passphrase becomes the canonical recovery secret for
    // every document in the workspace.
    if !tc.is_admin() {
        return Err(ApiError::Unauthorized);
    }
    // Validate wire shapes before we touch the DB — a malformed
    // salt / wrapped blob from the client should 400, not 500.
    let _ = orchext_crypto::Salt::from_wire(&req.kdf_salt)
        .map_err(|e| ApiError::InvalidArgument(format!("kdf_salt: {e}")))?;
    let _ = SealedBlob::from_wire(&req.wrapped_content_key)
        .map_err(|e| ApiError::InvalidArgument(format!("wrapped_content_key: {e}")))?;
    let _ = SealedBlob::from_wire(&req.key_check)
        .map_err(|e| ApiError::InvalidArgument(format!("key_check: {e}")))?;

    // UPDATE ... WHERE kdf_salt IS NULL makes init idempotent-forbidden:
    // once crypto is seeded, further calls to init-crypto no-op at the
    // row level and we return 409. This avoids a TOCTOU between the
    // check and the write.
    let affected = sqlx::query(
        r#"
        UPDATE tenants
        SET kdf_salt = $1,
            wrapped_content_key = $2,
            key_check_blob = $3,
            key_version = 1
        WHERE id = $4 AND kdf_salt IS NULL
        "#,
    )
    .bind(&req.kdf_salt)
    .bind(&req.wrapped_content_key)
    .bind(&req.key_check)
    .bind(tc.tenant_id)
    .execute(&state.db)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(ApiError::Conflict("crypto_already_seeded"));
    }
    Ok((StatusCode::CREATED, Json(InitCryptoResponse { key_version: 1 })))
}

// ---------- POST /session-key ----------

#[derive(Debug, Deserialize)]
struct PublishRequest {
    /// Raw content key, base64url-nopad of the 32 key bytes. Crossed
    /// on the wire under TLS; held only in server memory.
    key: String,
}

#[derive(Debug, Serialize)]
struct PublishResponse {
    expires_at: DateTime<Utc>,
    ttl_seconds: i64,
}

async fn publish_session_key(
    State(state): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    Extension(sc): Extension<SessionContext>,
    Json(req): Json<PublishRequest>,
) -> Result<Json<PublishResponse>, ApiError> {
    // Pull both the seeded flag and the stored key-check blob in one
    // query so we never publish a key that bypasses the verifier.
    let row: (bool, Option<String>) = sqlx::query_as(
        "SELECT (kdf_salt IS NOT NULL), key_check_blob FROM tenants WHERE id = $1",
    )
    .bind(tc.tenant_id)
    .fetch_one(&state.db)
    .await?;
    let (seeded, key_check) = row;
    if !seeded {
        return Err(ApiError::InvalidArgument(
            "crypto_not_seeded: call /vault/init-crypto first".into(),
        ));
    }
    // A seeded tenant must have a key-check blob from `init-crypto`.
    // If it's missing the row is from a pre-hardening seed; refuse to
    // publish rather than fall back to "any 32 bytes will do".
    let Some(key_check_wire) = key_check else {
        return Err(ApiError::InvalidArgument(
            "key_check_missing: tenant must be re-seeded".into(),
        ));
    };
    let key_check_blob = SealedBlob::from_wire(&key_check_wire)
        .map_err(|_| ApiError::Internal("stored key_check blob is corrupt".into()))?;

    let content_key = ContentKey::from_wire(&req.key)
        .map_err(|e| ApiError::InvalidArgument(format!("key: {e}")))?;

    // The core check. Any failure — bad bytes, blob tampered, plaintext
    // mismatch — comes back as a single tag so callers can't probe the
    // store for hints about what shape of key the verifier expects.
    verify_key_check(&key_check_blob, &content_key)
        .map_err(|_| ApiError::InvalidArgument("key_mismatch".into()))?;

    let outcome = state.session_keys.publish(
        tc.tenant_id,
        sc.session_id,
        *content_key.expose_bytes(),
        DEFAULT_TTL,
    );
    Ok(Json(PublishResponse {
        expires_at: outcome.expires_at,
        ttl_seconds: DEFAULT_TTL.num_seconds(),
    }))
}

async fn revoke_session_key(
    State(state): State<AppState>,
    Extension(tc): Extension<TenantContext>,
    Extension(sc): Extension<SessionContext>,
) -> Result<StatusCode, ApiError> {
    state.session_keys.revoke(tc.tenant_id, sc.session_id);
    Ok(StatusCode::NO_CONTENT)
}
