//! Server configuration, read from environment variables at startup.
//!
//! Only two env vars are required: `DATABASE_URL` and `ORCHEXT_BIND`.
//! Everything else has a reasonable default so `docker compose up`
//! works on first boot.

use std::env;

/// Drives the signup flow's org-assignment rule (Phase 3 D17d).
/// Self-hosted: first signup → owner of the singleton org; subsequent
/// signups → pending_signups for that singleton. SaaS: first signup of
/// a new email domain → owner of a new org claiming that domain;
/// matching-domain signups → pending for the existing org.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentMode {
    SelfHosted,
    Saas,
}

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
    /// `self_hosted` (default) or `saas`. See `DeploymentMode`.
    pub deployment_mode: DeploymentMode,
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
        let deployment_mode = match env::var("ORCHEXT_DEPLOYMENT_MODE")
            .ok()
            .as_deref()
        {
            Some("saas") => DeploymentMode::Saas,
            None | Some("") | Some("self_hosted") => DeploymentMode::SelfHosted,
            Some(other) => {
                return Err(ConfigError::InvalidDeploymentMode(other.to_string()))
            }
        };

        Ok(Config {
            database_url,
            bind,
            db_max_connections,
            secure_cookies,
            cors_allow_origins,
            deployment_mode,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("required environment variable not set: {0}")]
    Missing(&'static str),
    #[error("ORCHEXT_DEPLOYMENT_MODE must be 'self_hosted' or 'saas', got '{0}'")]
    InvalidDeploymentMode(String),
}
