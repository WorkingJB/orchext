//! OAuth discovery endpoints — JSON-shape end-to-end tests.
//!
//! Hits the full router so the routes are mounted at the right paths
//! (root well-known) and each handler emits the fields RFC 8414 / 9728
//! require for an MCP client to bootstrap the redirect flow.
//!
//! `sqlx::test` is the consistent pattern across this crate even
//! though these handlers don't query Postgres — `AppState` carries a
//! pool, and `connect_lazy` would still need a Tokio context to
//! construct, so giving the test a real DB is the simpler call.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use orchext_server::{router, AppState};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

const MAX_BODY: usize = 64 * 1024;

async fn read_json(body: Body) -> Value {
    let bytes = to_bytes(body, MAX_BODY).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn get(path: &str, host: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .header("host", host)
        .header("x-forwarded-proto", "https")
        .body(Body::empty())
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn authorization_server_metadata_carries_required_fields(db: PgPool) {
    let app = router(AppState::new(db).with_rate_limit_auth(false));
    let resp = app
        .clone()
        .oneshot(get("/.well-known/oauth-authorization-server", "app.example.org"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = read_json(resp.into_body()).await;

    let issuer = body["issuer"].as_str().unwrap();
    assert_eq!(issuer, "https://app.example.org");
    assert_eq!(
        body["authorization_endpoint"],
        "https://app.example.org/v1/oauth/authorize"
    );
    assert_eq!(
        body["token_endpoint"],
        "https://app.example.org/v1/oauth/token"
    );
    assert_eq!(
        body["registration_endpoint"],
        "https://app.example.org/v1/oauth/register"
    );

    let pkce = body["code_challenge_methods_supported"].as_array().unwrap();
    assert_eq!(pkce.len(), 1);
    assert_eq!(pkce[0], "S256");

    let auth_methods = body["token_endpoint_auth_methods_supported"]
        .as_array()
        .unwrap();
    let strs: Vec<&str> = auth_methods.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(strs.contains(&"client_secret_basic"));
    assert!(strs.contains(&"none"));
}

#[sqlx::test(migrations = "./migrations")]
async fn protected_resource_metadata_points_at_authorization_server(db: PgPool) {
    let app = router(AppState::new(db).with_rate_limit_auth(false));
    let resp = app
        .clone()
        .oneshot(get(
            "/.well-known/oauth-protected-resource",
            "app.example.org",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = read_json(resp.into_body()).await;
    assert_eq!(body["resource"], "https://app.example.org/v1/mcp");
    let servers = body["authorization_servers"].as_array().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0], "https://app.example.org");
}

#[sqlx::test(migrations = "./migrations")]
async fn protected_resource_metadata_responds_at_rfc9728_path_suffix(db: PgPool) {
    // RFC 9728 §3.1: clients construct the metadata URL by inserting
    // /.well-known/oauth-protected-resource between the host and the
    // resource path. For the MCP resource at /v1/mcp, that means a
    // probe at /.well-known/oauth-protected-resource/v1/mcp must
    // resolve to the same metadata.
    let app = router(AppState::new(db).with_rate_limit_auth(false));
    let resp = app
        .clone()
        .oneshot(get(
            "/.well-known/oauth-protected-resource/v1/mcp",
            "app.example.org",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = read_json(resp.into_body()).await;
    assert_eq!(body["resource"], "https://app.example.org/v1/mcp");
}

#[sqlx::test(migrations = "./migrations")]
async fn configured_base_url_overrides_request_host(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_base_url(Some("https://canonical.orchext.ai".to_string())),
    );
    let resp = app
        .clone()
        .oneshot(get(
            "/.well-known/oauth-authorization-server",
            "any-other-host.example.com",
        ))
        .await
        .unwrap();
    let body = read_json(resp.into_body()).await;
    assert_eq!(body["issuer"], "https://canonical.orchext.ai");
}
