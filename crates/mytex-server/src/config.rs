//! Server configuration, read from environment variables at startup.
//!
//! Only two env vars are required: `DATABASE_URL` and `MYTEX_BIND`.
//! Everything else has a reasonable default so `docker compose up`
//! works on first boot.

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: String,
    pub db_max_connections: u32,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("DATABASE_URL")
            .map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let bind = env::var("MYTEX_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let db_max_connections = env::var("MYTEX_DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        Ok(Config {
            database_url,
            bind,
            db_max_connections,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("required environment variable not set: {0}")]
    Missing(&'static str),
}
