//! Tauri-managed state: currently-open vault + its services.
//!
//! `AppState` is created empty at launch; `vault_open` populates it.
//! All commands that touch the vault go through `services()` which
//! errors cleanly if no vault is open yet (first-run / between
//! vault switches).

use mytex_audit::AuditWriter;
use mytex_auth::TokenService;
use mytex_index::Index;
use mytex_vault::{PlainFileDriver, VaultDriver};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct OpenVault {
    pub root: PathBuf,
    pub vault: Arc<dyn VaultDriver>,
    pub index: Arc<Index>,
    pub auth: Arc<TokenService>,
    pub audit: Arc<AuditWriter>,
}

#[derive(Default)]
pub struct AppState {
    inner: RwLock<Option<OpenVault>>,
}

impl AppState {
    pub async fn set(&self, v: OpenVault) {
        *self.inner.write().await = Some(v);
    }

    pub async fn root(&self) -> Option<PathBuf> {
        self.inner.read().await.as_ref().map(|v| v.root.clone())
    }

    pub async fn services(&self) -> Result<Services, String> {
        let guard = self.inner.read().await;
        let v = guard
            .as_ref()
            .ok_or_else(|| "no vault open".to_string())?;
        Ok(Services {
            root: v.root.clone(),
            vault: v.vault.clone(),
            index: v.index.clone(),
            auth: v.auth.clone(),
            audit: v.audit.clone(),
        })
    }
}

/// A snapshot of the handles needed to serve a single command, cloned
/// out from under the state lock. Cloning `Arc`s is cheap; this lets
/// commands do long-running work without holding the state lock.
pub struct Services {
    pub root: PathBuf,
    pub vault: Arc<dyn VaultDriver>,
    pub index: Arc<Index>,
    pub auth: Arc<TokenService>,
    /// Held so the writer stays alive during a command (any call that
    /// appends to the audit log goes through this handle via the future
    /// MCP-in-process bridge). Not dereferenced by today's commands, but
    /// dropping it here would sever that link the first time we add one.
    #[allow(dead_code)]
    pub audit: Arc<AuditWriter>,
}

/// Build the full service stack for a vault at `root`. Creates the
/// vault + `.mytex/` skeleton if missing, opens persistent stores,
/// and runs a full reindex so search/list reflect what's on disk.
pub async fn open_vault(root: PathBuf) -> Result<OpenVault, String> {
    // Canonicalize so fs-watch paths line up (matches mytex-mcp's
    // behavior on macOS where `/tmp` is a symlink).
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|e| format!("create vault dir: {e}"))?;
    let root = root
        .canonicalize()
        .map_err(|e| format!("canonicalize: {e}"))?;

    let mytex_dir = root.join(".mytex");
    tokio::fs::create_dir_all(&mytex_dir)
        .await
        .map_err(|e| format!("create .mytex: {e}"))?;

    // Seed type directories so an empty vault still has a navigable
    // shape for the UI. Matches `mytex-mcp init`.
    for t in SEED_TYPES {
        tokio::fs::create_dir_all(root.join(t))
            .await
            .map_err(|e| format!("create {t}: {e}"))?;
    }

    let vault: Arc<dyn VaultDriver> = Arc::new(PlainFileDriver::new(root.clone()));
    let index = Arc::new(
        Index::open(mytex_dir.join("index.sqlite"))
            .await
            .map_err(|e| format!("open index: {e}"))?,
    );
    index
        .reindex_from(&*vault)
        .await
        .map_err(|e| format!("reindex: {e}"))?;
    let auth = Arc::new(
        TokenService::open(mytex_dir.join("tokens.json"))
            .await
            .map_err(|e| format!("open tokens: {e}"))?,
    );
    let audit = Arc::new(
        AuditWriter::open(mytex_dir.join("audit.jsonl"))
            .await
            .map_err(|e| format!("open audit: {e}"))?,
    );

    Ok(OpenVault {
        root,
        vault,
        index,
        auth,
        audit,
    })
}

const SEED_TYPES: &[&str] = &[
    "identity",
    "roles",
    "goals",
    "relationships",
    "memories",
    "tools",
    "preferences",
    "domains",
    "decisions",
    "attachments",
];
