//! Tauri commands invoked by the frontend. Each is a thin wrapper
//! around the mytex-* crates, returning serde-serializable DTOs so
//! the UI doesn't need to know about internal types.

use crate::state::{self, AppState};
use chrono::{DateTime, Duration, Utc};
use mytex_audit::{verify, AuditEntry, Iter as AuditIter};
use mytex_auth::{IssueRequest, Mode, Scope};
use mytex_vault::{Document, DocumentId, Frontmatter, Visibility};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tauri::State;

// ---------------- vault ----------------

#[derive(Debug, Serialize)]
pub struct VaultInfo {
    pub root: String,
    pub document_count: u64,
}

#[tauri::command]
pub async fn vault_open(
    state: State<'_, AppState>,
    path: String,
) -> Result<VaultInfo, String> {
    let opened = state::open_vault(path.into()).await?;
    let root = opened.root.to_string_lossy().to_string();
    let list = opened
        .vault
        .list(None)
        .await
        .map_err(|e| format!("list: {e}"))?;
    let count = list.len() as u64;
    state.set(opened).await;
    Ok(VaultInfo {
        root,
        document_count: count,
    })
}

#[tauri::command]
pub async fn vault_info(state: State<'_, AppState>) -> Result<Option<VaultInfo>, String> {
    let Some(root) = state.root().await else {
        return Ok(None);
    };
    let svcs = state.services().await?;
    let list = svcs
        .vault
        .list(None)
        .await
        .map_err(|e| format!("list: {e}"))?;
    Ok(Some(VaultInfo {
        root: root.to_string_lossy().to_string(),
        document_count: list.len() as u64,
    }))
}

// ---------------- documents ----------------

#[derive(Debug, Serialize)]
pub struct DocListItem {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub title: String,
    pub visibility: String,
    pub tags: Vec<String>,
    pub updated: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DocDetail {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub visibility: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub aliases: Vec<String>,
    pub source: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub body: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct DocInput {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub visibility: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub body: String,
}

#[tauri::command]
pub async fn doc_list(state: State<'_, AppState>) -> Result<Vec<DocListItem>, String> {
    let svcs = state.services().await?;
    let entries = svcs
        .vault
        .list(None)
        .await
        .map_err(|e| format!("list: {e}"))?;

    // Pull each doc's frontmatter so the list reflects visibility, tags,
    // updated, and a sensible title. For v1 vault sizes (hundreds of
    // docs) the O(n) reads are fine; swap to an index-only path if this
    // ever gets heavy.
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        match svcs.vault.read(&entry.id).await {
            Ok(doc) => {
                let title = title_from_body(&doc.body, entry.id.as_str());
                out.push(DocListItem {
                    id: entry.id.to_string(),
                    type_: entry.type_,
                    title,
                    visibility: doc.frontmatter.visibility.as_label().to_string(),
                    tags: doc.frontmatter.tags,
                    updated: doc.frontmatter.updated.map(|d| d.to_string()),
                });
            }
            Err(e) => {
                tracing::warn!(id = %entry.id, err = %e, "skipping unreadable doc");
            }
        }
    }
    out.sort_by(|a, b| {
        b.updated
            .cmp(&a.updated)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(out)
}

#[tauri::command]
pub async fn doc_read(
    state: State<'_, AppState>,
    id: String,
) -> Result<DocDetail, String> {
    let svcs = state.services().await?;
    let doc_id = DocumentId::new(id).map_err(|e| e.to_string())?;
    let doc = svcs
        .vault
        .read(&doc_id)
        .await
        .map_err(|e| format!("read: {e}"))?;
    let version = doc.version().map_err(|e| e.to_string())?;
    Ok(DocDetail {
        id: doc.frontmatter.id.to_string(),
        type_: doc.frontmatter.type_.clone(),
        visibility: doc.frontmatter.visibility.as_label().to_string(),
        tags: doc.frontmatter.tags,
        links: doc.frontmatter.links,
        aliases: doc.frontmatter.aliases,
        source: doc.frontmatter.source,
        created: doc.frontmatter.created.map(|d| d.to_string()),
        updated: doc.frontmatter.updated.map(|d| d.to_string()),
        body: doc.body,
        version,
    })
}

#[tauri::command]
pub async fn doc_write(
    state: State<'_, AppState>,
    input: DocInput,
) -> Result<DocDetail, String> {
    let svcs = state.services().await?;
    let id = DocumentId::new(input.id.clone()).map_err(|e| format!("id: {e}"))?;
    let visibility = Visibility::from_label(&input.visibility)
        .map_err(|e| format!("visibility: {e}"))?;
    let today = Utc::now().date_naive();

    // Preserve `created` from disk if the doc already exists; stamp
    // `updated` to today on every write.
    let existing = svcs.vault.read(&id).await.ok();
    let created = existing
        .as_ref()
        .and_then(|d| d.frontmatter.created)
        .or(Some(today));

    let fm = Frontmatter {
        id: id.clone(),
        type_: input.type_.clone(),
        visibility,
        tags: input.tags,
        links: input.links,
        aliases: input.aliases,
        created,
        updated: Some(today),
        source: input.source,
        principal: None,
        schema: None,
        extras: BTreeMap::new(),
    };
    let doc = Document {
        frontmatter: fm,
        body: input.body,
    };
    svcs.vault
        .write(&id, &doc)
        .await
        .map_err(|e| format!("write: {e}"))?;
    svcs.index
        .upsert(&input.type_, &doc)
        .await
        .map_err(|e| format!("index upsert: {e}"))?;

    let version = doc.version().map_err(|e| e.to_string())?;
    Ok(DocDetail {
        id: id.to_string(),
        type_: doc.frontmatter.type_.clone(),
        visibility: doc.frontmatter.visibility.as_label().to_string(),
        tags: doc.frontmatter.tags,
        links: doc.frontmatter.links,
        aliases: doc.frontmatter.aliases,
        source: doc.frontmatter.source,
        created: doc.frontmatter.created.map(|d| d.to_string()),
        updated: doc.frontmatter.updated.map(|d| d.to_string()),
        body: doc.body,
        version,
    })
}

#[tauri::command]
pub async fn doc_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let svcs = state.services().await?;
    let doc_id = DocumentId::new(id).map_err(|e| e.to_string())?;
    svcs.vault
        .delete(&doc_id)
        .await
        .map_err(|e| format!("delete: {e}"))?;
    svcs.index
        .remove(&doc_id)
        .await
        .map_err(|e| format!("index remove: {e}"))?;
    Ok(())
}

// ---------------- tokens ----------------

#[derive(Debug, Serialize)]
pub struct TokenInfo {
    pub id: String,
    pub label: String,
    pub scope: Vec<String>,
    pub mode: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
    pub revoked: bool,
}

#[derive(Debug, Serialize)]
pub struct IssuedTokenDto {
    pub info: TokenInfo,
    /// Shown to the user exactly once.
    pub secret: String,
}

#[derive(Debug, Deserialize)]
pub struct TokenIssueInput {
    pub label: String,
    pub scope: Vec<String>,
    pub mode: String, // "read" | "read_propose"
    pub ttl_days: Option<i64>,
}

#[tauri::command]
pub async fn token_list(state: State<'_, AppState>) -> Result<Vec<TokenInfo>, String> {
    let svcs = state.services().await?;
    let tokens = svcs.auth.list().await;
    Ok(tokens.iter().map(public_to_info).collect())
}

#[tauri::command]
pub async fn token_issue(
    state: State<'_, AppState>,
    input: TokenIssueInput,
) -> Result<IssuedTokenDto, String> {
    let svcs = state.services().await?;
    let scope = Scope::new(input.scope).map_err(|e| format!("scope: {e}"))?;
    let mode = match input.mode.as_str() {
        "read" => Mode::Read,
        "read_propose" => Mode::ReadPropose,
        other => return Err(format!("unknown mode: {other}")),
    };
    let ttl = input.ttl_days.map(Duration::days);
    let issued = svcs
        .auth
        .issue(IssueRequest {
            label: input.label,
            scope,
            mode,
            limits: Default::default(),
            ttl,
        })
        .await
        .map_err(|e| format!("issue: {e}"))?;
    Ok(IssuedTokenDto {
        info: public_to_info(&issued.info),
        secret: issued.secret.expose().to_string(),
    })
}

#[tauri::command]
pub async fn token_revoke(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let svcs = state.services().await?;
    svcs.auth
        .revoke(&id)
        .await
        .map_err(|e| format!("revoke: {e}"))
}

// ---------------- audit ----------------

#[derive(Debug, Serialize)]
pub struct AuditRow {
    pub seq: u64,
    pub ts: DateTime<Utc>,
    pub actor: String,
    pub action: String,
    pub document_id: Option<String>,
    pub scope_used: Vec<String>,
    pub outcome: String,
}

#[derive(Debug, Serialize)]
pub struct AuditPage {
    pub entries: Vec<AuditRow>,
    pub total: u64,
    pub chain_valid: bool,
}

#[tauri::command]
pub async fn audit_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<AuditPage, String> {
    let svcs = state.services().await?;
    let path = svcs.root.join(".mytex/audit.jsonl");
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(AuditPage {
            entries: vec![],
            total: 0,
            chain_valid: true,
        });
    }

    let report = verify(&path).await.ok();
    let chain_valid = report.is_some();
    let total = report.as_ref().map(|r| r.total_entries).unwrap_or(0);

    let mut iter = AuditIter::open(&path).await.map_err(|e| e.to_string())?;
    let mut all = Vec::new();
    while let Some(entry) = iter.next().await.map_err(|e| e.to_string())? {
        all.push(entry);
    }
    // Newest first.
    all.reverse();
    if let Some(n) = limit {
        all.truncate(n);
    }
    let entries = all.into_iter().map(entry_to_row).collect();
    Ok(AuditPage {
        entries,
        total,
        chain_valid,
    })
}

// ---------------- helpers ----------------

fn title_from_body(body: &str, fallback_id: &str) -> String {
    for line in body.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("# ") {
            let s = rest.trim();
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    fallback_id.to_string()
}

fn public_to_info(t: &mytex_auth::PublicTokenInfo) -> TokenInfo {
    TokenInfo {
        id: t.id.clone(),
        label: t.label.clone(),
        scope: t.scope.clone(),
        mode: match t.mode {
            Mode::Read => "read".into(),
            Mode::ReadPropose => "read_propose".into(),
        },
        created_at: t.created_at,
        expires_at: t.expires_at,
        last_used: t.last_used,
        revoked: t.revoked_at.is_some(),
    }
}

fn entry_to_row(e: AuditEntry) -> AuditRow {
    AuditRow {
        seq: e.seq,
        ts: e.ts,
        actor: e.actor.as_encoded(),
        action: e.action,
        document_id: e.document_id,
        scope_used: e.scope_used,
        outcome: format!("{:?}", e.outcome).to_lowercase(),
    }
}

