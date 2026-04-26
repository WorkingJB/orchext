//! Server configuration, read from environment variables at startup.
//!
//! Only two env vars are required: `DATABASE_URL` and `ORCHEXT_BIND`.
//! Everything else has a reasonable default so `docker compose up`
//! works on first boot.

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: String,
    pub db_max_connections: u32,
    /// Whether to issue cookies with the `Secure` flag. Defaults to
    /// `true` (production). Local HTTP dev needs `ORCHEXT_SECURE_COOKIES=0`
    /// or browsers will silently drop the cookie.
    pub secure_cookies: bool,
    /// Origins allowed to make credentialed cross-origin requests.
    /// Empty (the default) means **no CORS layer is mounted** —
    /// cross-origin browsers get the standard browser-side block, no
    /// preflight is answered, no headers are echoed. The hosted SaaS
    /// deployment is same-origin via Vercel rewrites and leaves this
    /// empty; self-hosters who serve the web app from a different
    /// origin set the comma-separated list here.
    pub cors_allow_origins: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("DATABASE_URL")
            .map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let bind = env::var("ORCHEXT_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let db_max_connections = env::var("ORCHEXT_DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);
        let secure_cookies = env::var("ORCHEXT_SECURE_COOKIES")
            .ok()
            .map(|s| !matches!(s.as_str(), "0" | "false" | "no"))
            .unwrap_or(true);
        let cors_allow_origins = env::var("ORCHEXT_CORS_ALLOW_ORIGINS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|o| o.trim().to_string())
                    .filter(|o| !o.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Config {
            database_url,
            bind,
            db_max_connections,
            secure_cookies,
            cors_allow_origins,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("required environment variable not set: {0}")]
    Missing(&'static str),
}
