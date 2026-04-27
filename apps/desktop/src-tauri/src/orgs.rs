//! Tauri commands wrapping `/v1/orgs/*` and `/v1/auth/me|logout` for
//! a remote workspace. Each command takes a `workspace_id` so the
//! frontend can target a specific server (one workspace = one server +
//! one tenant; the org/member surface is server-scoped, not tenant-
//! scoped, so a single `workspace_id` is enough to look up the right
//! `(server_url, session_token)` pair).
//!
//! Phase 3 platform Slice 1 (D17a, D17d, D17f, D17g).
//!
//! All bodies use camelCase from TS via Tauri's default serde rename.
//! The DTOs themselves come from `orchext_sync::orgs` so we don't
//! restate the wire shapes — server-side renames flow through.

use crate::state::AppState;
use crate::workspaces::WorkspaceEntry;
use orchext_sync::orgs;
use serde::Deserialize;
use tauri::State;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct ServerCreds {
    server_url: Url,
    session_token: String,
}

/// Look up a remote workspace by id and return its `(server_url,
/// session_token)`. Errors if the workspace is local or the id is
/// unknown — callers should never invoke org commands against local
/// workspaces (no server, nothing to ask).
async fn server_creds(state: &AppState, workspace_id: &str) -> Result<ServerCreds, String> {
    let reg = state.registry_snapshot().await;
    let entry: &WorkspaceEntry = reg
        .find(workspace_id)
        .ok_or_else(|| format!("unknown workspace: {workspace_id}"))?;
    if entry.kind != "remote" {
        return Err("local workspaces have no org surface".into());
    }
    let server_url = entry
        .server_url
        .as_deref()
        .ok_or_else(|| "remote workspace missing server_url".to_string())?
        .parse::<Url>()
        .map_err(|e| format!("invalid server url: {e}"))?;
    let session_token = entry
        .session_token
        .clone()
        .ok_or_else(|| "remote workspace has no session token; reconnect".to_string())?;
    Ok(ServerCreds {
        server_url,
        session_token,
    })
}

fn err(e: orchext_sync::SyncError) -> String {
    e.to_string()
}

// ---------- /v1/auth ----------

#[tauri::command]
pub async fn auth_me(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<orgs::MeResponse, String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::auth_me(&c.server_url, &c.session_token)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn auth_logout(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<(), String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::auth_logout(&c.server_url, &c.session_token)
        .await
        .map_err(err)
}

// ---------- /v1/orgs ----------

#[tauri::command]
pub async fn orgs_list(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<orgs::OrgsListResponse, String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::orgs_list(&c.server_url, &c.session_token)
        .await
        .map_err(err)
}

#[derive(Debug, Deserialize)]
pub struct CreateOrgInput {
    pub name: String,
}

#[tauri::command]
pub async fn org_create(
    state: State<'_, AppState>,
    workspace_id: String,
    input: CreateOrgInput,
) -> Result<orgs::Organization, String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_create(&c.server_url, &c.session_token, input.name.trim())
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn org_get(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
) -> Result<orgs::Organization, String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_get(&c.server_url, &c.session_token, org_id)
        .await
        .map_err(err)
}

/// Match the web's `UpdateOrgInput` shape — every field optional so
/// the same command handles partial patches (e.g., name-only rename).
#[derive(Debug, Default, Deserialize)]
pub struct UpdateOrgInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub logo_url: Option<String>,
    #[serde(default)]
    pub allowed_domains: Option<Vec<String>>,
    #[serde(default)]
    pub settings: Option<serde_json::Value>,
}

#[tauri::command]
pub async fn org_update(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    input: UpdateOrgInput,
) -> Result<orgs::Organization, String> {
    let c = server_creds(&state, &workspace_id).await?;
    let body = orgs::UpdateOrgInput {
        name: input.name,
        logo_url: input.logo_url,
        allowed_domains: input.allowed_domains,
        settings: input.settings,
    };
    orgs::org_update(&c.server_url, &c.session_token, org_id, &body)
        .await
        .map_err(err)
}

// ---------- /v1/orgs/:id/members ----------

#[tauri::command]
pub async fn org_members(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
) -> Result<orgs::MembersResponse, String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_members(&c.server_url, &c.session_token, org_id)
        .await
        .map_err(err)
}

#[derive(Debug, Deserialize)]
pub struct PatchMemberInput {
    pub role: String,
}

#[tauri::command]
pub async fn org_member_update(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    account_id: Uuid,
    input: PatchMemberInput,
) -> Result<orgs::MemberDetail, String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_member_update(
        &c.server_url,
        &c.session_token,
        org_id,
        account_id,
        input.role.trim(),
    )
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn org_member_remove(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    account_id: Uuid,
) -> Result<(), String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_member_remove(&c.server_url, &c.session_token, org_id, account_id)
        .await
        .map_err(err)
}

// ---------- /v1/orgs/:id/pending ----------

#[tauri::command]
pub async fn org_pending(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    status: Option<String>,
) -> Result<orgs::PendingResponse, String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_pending(
        &c.server_url,
        &c.session_token,
        org_id,
        status.as_deref(),
    )
    .await
    .map_err(err)
}

#[derive(Debug, Default, Deserialize)]
pub struct ApproveInput {
    #[serde(default)]
    pub role: Option<String>,
}

#[tauri::command]
pub async fn org_pending_approve(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    account_id: Uuid,
    input: Option<ApproveInput>,
) -> Result<orgs::MemberDetail, String> {
    let c = server_creds(&state, &workspace_id).await?;
    let role = input.and_then(|i| i.role);
    orgs::org_pending_approve(
        &c.server_url,
        &c.session_token,
        org_id,
        account_id,
        role.as_deref(),
    )
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn org_pending_reject(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    account_id: Uuid,
) -> Result<(), String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_pending_reject(&c.server_url, &c.session_token, org_id, account_id)
        .await
        .map_err(err)
}

// ---------- /v1/orgs/:id/invitations ----------

#[tauri::command]
pub async fn org_invitations(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    status: Option<String>,
) -> Result<orgs::InvitationsResponse, String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_invitations(
        &c.server_url,
        &c.session_token,
        org_id,
        status.as_deref(),
    )
    .await
    .map_err(err)
}

#[derive(Debug, Deserialize)]
pub struct CreateInvitationInput {
    pub email: String,
    #[serde(default)]
    pub role: Option<String>,
}

#[tauri::command]
pub async fn org_invite(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    input: CreateInvitationInput,
) -> Result<orgs::Invitation, String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_invite(
        &c.server_url,
        &c.session_token,
        org_id,
        input.email.trim(),
        input.role.as_deref(),
    )
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn org_invitation_delete(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    invitation_id: Uuid,
) -> Result<(), String> {
    let c = server_creds(&state, &workspace_id).await?;
    orgs::org_invitation_delete(
        &c.server_url,
        &c.session_token,
        org_id,
        invitation_id,
    )
    .await
    .map_err(err)
}
