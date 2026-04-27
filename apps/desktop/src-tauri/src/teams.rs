//! Tauri commands wrapping `/v1/orgs/:org_id/teams[/...]` for
//! Phase 3 platform Slice 2. Mirrors `orgs.rs` exactly — same
//! `(server_url, session_token)` lookup, same DTO sourcing from
//! `orchext_sync::teams`.

use crate::state::AppState;
use crate::workspaces::WorkspaceEntry;
use orchext_sync::teams;
use serde::Deserialize;
use tauri::State;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct ServerCreds {
    server_url: Url,
    session_token: String,
}

async fn server_creds(state: &AppState, workspace_id: &str) -> Result<ServerCreds, String> {
    let reg = state.registry_snapshot().await;
    let entry: &WorkspaceEntry = reg
        .find(workspace_id)
        .ok_or_else(|| format!("unknown workspace: {workspace_id}"))?;
    if entry.kind != "remote" {
        return Err("local workspaces have no team surface".into());
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

#[tauri::command]
pub async fn teams_list(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
) -> Result<teams::TeamsListResponse, String> {
    let c = server_creds(&state, &workspace_id).await?;
    teams::teams_list(&c.server_url, &c.session_token, org_id)
        .await
        .map_err(err)
}

#[derive(Debug, Deserialize)]
pub struct CreateTeamInput {
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
}

#[tauri::command]
pub async fn team_create(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    input: CreateTeamInput,
) -> Result<teams::Team, String> {
    let c = server_creds(&state, &workspace_id).await?;
    teams::team_create(
        &c.server_url,
        &c.session_token,
        org_id,
        input.name.trim(),
        input.slug.as_deref(),
    )
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn team_get(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    team_id: Uuid,
) -> Result<teams::Team, String> {
    let c = server_creds(&state, &workspace_id).await?;
    teams::team_get(&c.server_url, &c.session_token, org_id, team_id)
        .await
        .map_err(err)
}

#[derive(Debug, Default, Deserialize)]
pub struct UpdateTeamInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
}

#[tauri::command]
pub async fn team_update(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    team_id: Uuid,
    input: UpdateTeamInput,
) -> Result<teams::Team, String> {
    let c = server_creds(&state, &workspace_id).await?;
    let body = teams::UpdateTeamInput {
        name: input.name,
        slug: input.slug,
    };
    teams::team_update(&c.server_url, &c.session_token, org_id, team_id, &body)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn team_delete(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    team_id: Uuid,
) -> Result<(), String> {
    let c = server_creds(&state, &workspace_id).await?;
    teams::team_delete(&c.server_url, &c.session_token, org_id, team_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn team_members(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    team_id: Uuid,
) -> Result<teams::TeamMembersResponse, String> {
    let c = server_creds(&state, &workspace_id).await?;
    teams::team_members(&c.server_url, &c.session_token, org_id, team_id)
        .await
        .map_err(err)
}

#[derive(Debug, Deserialize)]
pub struct AddTeamMemberInput {
    pub account_id: Uuid,
    #[serde(default)]
    pub role: Option<String>,
}

#[tauri::command]
pub async fn team_member_add(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    team_id: Uuid,
    input: AddTeamMemberInput,
) -> Result<teams::TeamMemberDetail, String> {
    let c = server_creds(&state, &workspace_id).await?;
    teams::team_member_add(
        &c.server_url,
        &c.session_token,
        org_id,
        team_id,
        input.account_id,
        input.role.as_deref(),
    )
    .await
    .map_err(err)
}

#[derive(Debug, Deserialize)]
pub struct PatchTeamMemberInput {
    pub role: String,
}

#[tauri::command]
pub async fn team_member_update(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    team_id: Uuid,
    account_id: Uuid,
    input: PatchTeamMemberInput,
) -> Result<teams::TeamMemberDetail, String> {
    let c = server_creds(&state, &workspace_id).await?;
    teams::team_member_update(
        &c.server_url,
        &c.session_token,
        org_id,
        team_id,
        account_id,
        input.role.trim(),
    )
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn team_member_remove(
    state: State<'_, AppState>,
    workspace_id: String,
    org_id: Uuid,
    team_id: Uuid,
    account_id: Uuid,
) -> Result<(), String> {
    let c = server_creds(&state, &workspace_id).await?;
    teams::team_member_remove(
        &c.server_url,
        &c.session_token,
        org_id,
        team_id,
        account_id,
    )
    .await
    .map_err(err)
}
