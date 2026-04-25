//! Per-vault settings stored in `.orchext/settings.json`.
//!
//! Today this only holds the Anthropic API key for the onboarding
//! agent. It lives alongside `tokens.json` and `audit.jsonl` so a
//! vault directory is self-contained.
//!
//! ⚠️ The API key is stored in plaintext on disk. For the MVP this
//! matches how `tokens.json` already handles secrets at rest (the
//! argon2-hashed token secrets excepted). Moving secrets to the OS
//! keychain is a follow-up — see implementation-status.md §desktop
//! Known gaps.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
}

pub async fn load(root: &Path) -> Result<Settings, String> {
    let path = settings_path(root);
    match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("parse settings.json: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(format!("read settings.json: {e}")),
    }
}

pub async fn save(root: &Path, settings: &Settings) -> Result<(), String> {
    let path = settings_path(root);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("create .orchext: {e}"))?;
    }
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|e| format!("encode settings: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, bytes)
        .await
        .map_err(|e| format!("write settings.json: {e}"))?;
    tokio::fs::rename(&tmp, &path)
        .await
        .map_err(|e| format!("rename settings.json: {e}"))?;
    Ok(())
}

fn settings_path(root: &Path) -> std::path::PathBuf {
    root.join(".orchext").join("settings.json")
}
