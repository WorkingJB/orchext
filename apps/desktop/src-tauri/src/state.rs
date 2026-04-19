//! Tauri-managed state: the workspace registry plus the currently-
//! active open vault.
//!
//! Phase 2a model: the registry tracks N workspaces; at any moment at
//! most one is *open* (its services loaded, watcher running). Switching
//! workspaces drops the previous `OpenVault` and opens a new one. This
//! is a deliberate simplification — keeping every workspace warm would
//! require N watchers, N indices in memory, and a coordination story
//! for the fs-watcher event channel, none of which is worth it at v1
//! vault sizes.

use crate::watch::WatcherHandle;
use crate::workspaces::{self, Registry, WorkspaceEntry};
use mytex_audit::AuditWriter;
use mytex_auth::TokenService;
use mytex_index::Index;
use mytex_vault::{PlainFileDriver, VaultDriver};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct OpenVault {
    pub workspace_id: String,
    pub root: PathBuf,
    pub vault: Arc<dyn VaultDriver>,
    pub index: Arc<Index>,
    pub auth: Arc<TokenService>,
    pub audit: Arc<AuditWriter>,
    /// Kept alive so the notify watcher thread doesn't exit. Replaced
    /// on each workspace switch (switching drops the old one).
    pub _watcher: Option<WatcherHandle>,
}

pub struct AppState {
    registry_path: PathBuf,
    registry: RwLock<Registry>,
    open: RwLock<Option<OpenVault>>,
}

impl AppState {
    pub async fn new(registry_path: PathBuf) -> Result<Self, String> {
        let registry = Registry::load(&registry_path).await?;
        Ok(AppState {
            registry_path,
            registry: RwLock::new(registry),
            open: RwLock::new(None),
        })
    }

    pub async fn registry_snapshot(&self) -> Registry {
        self.registry.read().await.clone()
    }

    /// Apply a mutation to the registry, then persist atomically. The
    /// mutation runs under the write lock so concurrent callers can't
    /// race. Saves the registry to disk before releasing the lock so a
    /// subsequent `registry_snapshot` always reflects what's on disk.
    pub async fn mutate_registry<F, T>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce(&mut Registry) -> Result<T, String>,
    {
        let mut g = self.registry.write().await;
        let out = f(&mut g)?;
        g.save(&self.registry_path).await?;
        Ok(out)
    }

    pub async fn is_active_open(&self, id: &str) -> bool {
        self.open
            .read()
            .await
            .as_ref()
            .map(|v| v.workspace_id == id)
            .unwrap_or(false)
    }

    /// Swap in a fresh `OpenVault`, dropping any previous one. Dropping
    /// the old `OpenVault` tears down its watcher.
    pub async fn set_open(&self, v: OpenVault) {
        *self.open.write().await = Some(v);
    }

    pub async fn clear_open(&self) {
        *self.open.write().await = None;
    }

    pub async fn active_services(&self) -> Result<Services, String> {
        let guard = self.open.read().await;
        let v = guard
            .as_ref()
            .ok_or_else(|| "no workspace open".to_string())?;
        Ok(Services {
            workspace_id: v.workspace_id.clone(),
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
    #[allow(dead_code)]
    pub workspace_id: String,
    pub root: PathBuf,
    pub vault: Arc<dyn VaultDriver>,
    pub index: Arc<Index>,
    pub auth: Arc<TokenService>,
    #[allow(dead_code)]
    pub audit: Arc<AuditWriter>,
}

/// Build the full service stack for a workspace. Creates the vault +
/// `.mytex/` skeleton if missing, opens persistent stores, and runs a
/// full reindex so search/list reflect what's on disk.
pub async fn open_workspace(entry: &WorkspaceEntry) -> Result<OpenVault, String> {
    if entry.kind != "local" {
        return Err(format!(
            "unsupported workspace kind for this phase: {}",
            entry.kind
        ));
    }
    let root = entry.path.clone();
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
        workspace_id: entry.id.clone(),
        root,
        vault,
        index,
        auth,
        audit,
        _watcher: None,
    })
}

/// Convenience constructor that uses the registry's default path.
pub async fn default_state() -> Result<AppState, String> {
    AppState::new(workspaces::default_registry_path()).await
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
