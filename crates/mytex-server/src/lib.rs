//! Mytex server — HTTP API.
//!
//! Today (Phase 2b.1) the surface is limited to user authentication:
//! signup, login, session middleware, logout. Vault and index endpoints
//! land in 2b.2; encryption in 2b.3; MCP HTTP/SSE + `context.propose`
//! in 2b.4. See `docs/implementation-status.md` §Phase 2b.

#![forbid(unsafe_code)]

pub mod accounts;
pub mod auth;
pub mod config;
pub mod error;
pub mod password;
pub mod sessions;

use axum::{routing::get, Router};
use sqlx::PgPool;
use std::sync::Arc;

/// Shared handle passed to every request handler.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub sessions: Arc<sessions::SessionService>,
}

impl AppState {
    pub fn new(db: PgPool) -> Self {
        let sessions = Arc::new(sessions::SessionService::new(db.clone()));
        AppState { db, sessions }
    }
}

/// Build the full `axum::Router` with every route mounted. Callers are
/// responsible for binding to an address and running the server — this
/// lets integration tests stand it up with `tower::ServiceExt` without
/// a real network listener.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .nest("/v1/auth", auth::router(state.clone()))
        .with_state(state)
}

async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "ok": true }))
}

/// Run embedded migrations against the provided pool. Called from
/// `main` on startup so the server is usable out of the box; tests
/// call it explicitly.
pub async fn migrate(db: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(db).await
}
