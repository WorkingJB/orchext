//! OAuth 2.1 + RFC 9728 discovery endpoints.
//!
//! Two routes mounted at the host root:
//! - `GET /.well-known/oauth-authorization-server` — RFC 8414 issuer
//!   metadata. Lets MCP clients discover the authorize / token /
//!   register endpoints from the issuer URL alone, which is what
//!   "paste the server URL into Claude/ChatGPT/Copilot" expands into
//!   under the hood.
//! - `GET /.well-known/oauth-protected-resource` (and the RFC 9728
//!   §3.1 strict path-suffixed variant) — resource metadata pointing
//!   at the authorization server. Returned the same regardless of the
//!   probed path because we only host one resource (`/v1/mcp`).
//!
//! Issuer URL resolution: prefer `AppState::base_url` (set from
//! `ORCHEXT_BASE_URL`) so SaaS deployments behind a load balancer
//! always advertise the canonical hostname. Fall back to
//! reconstructing from `X-Forwarded-Proto` + `Host` for self-hosters
//! who haven't set the env var. As a last resort assume `https://` —
//! plain-HTTP localhost dev still works because clients don't actually
//! consume the metadata in that mode.

use crate::AppState;
use axum::{
    extract::State,
    http::HeaderMap,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        // RFC 9728 §3.1: clients construct the metadata URL by inserting
        // `/.well-known/oauth-protected-resource` between the host and
        // the resource path. For the MCP resource at `/v1/mcp` and its
        // `/sse` alias, that produces `/.well-known/.../v1/mcp[/sse]`.
        .route(
            "/.well-known/oauth-protected-resource/*resource",
            get(protected_resource_metadata),
        )
}

async fn authorization_server_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<Value> {
    let issuer = issuer(state.base_url.as_deref(), &headers);
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/v1/oauth/authorize"),
        "token_endpoint": format!("{issuer}/v1/oauth/token"),
        "registration_endpoint": format!("{issuer}/v1/oauth/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code"],
        // OAuth 2.1 §7.5.2: only S256 is permitted; `plain` is forbidden.
        "code_challenge_methods_supported": ["S256"],
        // `none` covers the existing desktop POST-authorize codes which
        // carry no client_id; `client_secret_basic` covers redirect
        // codes minted for a registered client (3f.1 D47).
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "none"],
        // Coarse scopes — visibility labels (per-org `read` / `work` /
        // etc.) layer on top via the `/authorize` consent screen rather
        // than enumerating every label here.
        "scopes_supported": ["read", "read_propose"],
    }))
}

async fn protected_resource_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<Value> {
    let issuer = issuer(state.base_url.as_deref(), &headers);
    Json(json!({
        "resource": format!("{issuer}/v1/mcp"),
        "authorization_servers": [issuer.clone()],
        "bearer_methods_supported": ["header"],
    }))
}

/// Derive the canonical `https://host` issuer URL for this request.
/// Order of preference: configured `base_url` (env), then the
/// load-balancer-set forwarded headers, finally a permissive `Host`-
/// based reconstruction with `https` assumed for non-loopback hosts.
pub fn issuer(base_url: Option<&str>, headers: &HeaderMap) -> String {
    if let Some(base) = base_url {
        return base.trim_end_matches('/').to_string();
    }

    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            // No XFP. Default to https unless the Host header is a
            // loopback address — in that case the dev is almost
            // certainly running plain HTTP locally.
            let host = headers
                .get("host")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if host.starts_with("localhost")
                || host.starts_with("127.0.0.1")
                || host.starts_with("[::1]")
            {
                "http".to_string()
            } else {
                "https".to_string()
            }
        });

    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");

    format!("{scheme}://{host}")
}

#[cfg(test)]
mod tests {
    //! `issuer` covers four cases: configured base wins; XFP+host
    //! reconstructs; loopback host implies http; otherwise https.
    //! All four are reachable from the discovery handlers, which is
    //! why the helper is its own pub-crate function.

    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            let name = axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap();
            h.insert(name, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn issuer_prefers_configured_base_url() {
        let h = headers(&[("host", "ignored.example.com")]);
        // Trailing slash is normalized off so callers can append paths.
        assert_eq!(
            issuer(Some("https://app.orchext.ai/"), &h),
            "https://app.orchext.ai"
        );
    }

    #[test]
    fn issuer_uses_x_forwarded_proto_and_host() {
        let h = headers(&[
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "self.example.org"),
            ("host", "internal-pod-ip"),
        ]);
        assert_eq!(issuer(None, &h), "https://self.example.org");
    }

    #[test]
    fn issuer_assumes_http_for_loopback_host() {
        let h = headers(&[("host", "localhost:8080")]);
        assert_eq!(issuer(None, &h), "http://localhost:8080");
    }

    #[test]
    fn issuer_assumes_https_for_external_host_without_xfp() {
        let h = headers(&[("host", "self-hosted.example.org")]);
        assert_eq!(issuer(None, &h), "https://self-hosted.example.org");
    }
}
