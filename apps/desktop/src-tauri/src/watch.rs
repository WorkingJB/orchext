//! Filesystem watcher → live re-index + Tauri `vault://changed` event.
//!
//! Mirrors the pattern in `crates/orchext-mcp/src/watch.rs`: a sync thread
//! owns the `notify::RecommendedWatcher` receiver, classifies relevant
//! paths to `(type, id)`, then hops back onto the tokio runtime to
//! upsert/remove the index and emit a Tauri event the frontend can
//! listen for. No debouncing (matches mcp).

use orchext_index::Index;
use orchext_vault::{DocumentId, VaultDriver};
use notify::event::EventKind;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::runtime::Handle;

pub struct WatcherHandle {
    _watcher: RecommendedWatcher,
}

#[derive(Clone, serde::Serialize)]
pub struct VaultChanged {
    #[serde(rename = "type")]
    pub type_: String,
    pub id: String,
    pub kind: &'static str,
}

pub fn spawn(
    vault_root: PathBuf,
    vault: Arc<dyn VaultDriver>,
    index: Arc<Index>,
    app: AppHandle,
) -> Result<WatcherHandle, notify::Error> {
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(&vault_root, RecursiveMode::Recursive)?;

    let runtime = Handle::current();
    let root = vault_root;
    std::thread::Builder::new()
        .name("orchext-desktop-watch".into())
        .spawn(move || {
            while let Ok(res) = rx.recv() {
                match res {
                    Ok(event) => {
                        if !is_relevant_kind(&event.kind) {
                            continue;
                        }
                        for path in event.paths {
                            if let Some((type_, id)) = classify(&root, &path) {
                                let vault = vault.clone();
                                let index = index.clone();
                                let app = app.clone();
                                runtime.spawn(async move {
                                    apply_and_notify(&*vault, &index, &app, &type_, &id).await;
                                });
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, "fs watcher error");
                    }
                }
            }
        })
        .expect("spawn watcher thread");

    Ok(WatcherHandle { _watcher: watcher })
}

async fn apply_and_notify(
    vault: &dyn VaultDriver,
    index: &Index,
    app: &AppHandle,
    type_: &str,
    id: &str,
) {
    let Ok(doc_id) = DocumentId::new(id) else {
        tracing::debug!(id, "skipping watcher event with invalid id");
        return;
    };

    let kind = match vault.read(&doc_id).await {
        Ok(doc) => {
            if let Err(e) = index.upsert(type_, &doc).await {
                tracing::warn!(err = %e, id, "watcher upsert failed");
            }
            "upsert"
        }
        Err(_) => {
            if let Err(e) = index.remove(&doc_id).await {
                tracing::warn!(err = %e, id, "watcher remove failed");
            }
            "remove"
        }
    };

    if let Err(e) = app.emit(
        "vault://changed",
        VaultChanged {
            type_: type_.to_string(),
            id: id.to_string(),
            kind,
        },
    ) {
        tracing::warn!(err = %e, "emit vault://changed failed");
    }
}

fn is_relevant_kind(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

/// Map a path under `root` to `(type, id)`, or None if it's not a doc.
/// Shape must be `<root>/<type>/<id>.md`; `.orchext/`, dot-dirs, and deeper
/// nesting are ignored so behavior matches `PlainFileDriver::list`.
fn classify(root: &Path, path: &Path) -> Option<(String, String)> {
    let rel = path.strip_prefix(root).ok()?;
    let components: Vec<_> = rel.components().collect();
    if components.len() != 2 {
        return None;
    }
    let type_name = components[0].as_os_str().to_str()?;
    if type_name.starts_with('.') {
        return None;
    }
    let file = components[1].as_os_str().to_str()?;
    let id = file.strip_suffix(".md")?;
    if id.is_empty() {
        return None;
    }
    Some((type_name.to_string(), id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_doc_path() {
        let root = Path::new("/vault");
        assert_eq!(
            classify(root, Path::new("/vault/relationships/rel-jane.md")),
            Some(("relationships".into(), "rel-jane".into()))
        );
    }

    #[test]
    fn skips_dot_orchext() {
        let root = Path::new("/vault");
        assert!(classify(root, Path::new("/vault/.orchext/audit.jsonl")).is_none());
    }

    #[test]
    fn skips_non_md() {
        let root = Path::new("/vault");
        assert!(classify(root, Path::new("/vault/relationships/rel-jane.txt")).is_none());
    }
}
