//! Server-level team helpers for native clients.
//!
//! Mirrors `orchext-server::teams` request/response shapes. Like
//! `orgs.rs`, these are standalone functions taking `(server_url,
//! token, ...)` rather than `RemoteClient` methods because they live
//! at the server scope, not under a tenant-scoped `RemoteClient`.

use crate::client::translate_error;
use crate::error::Result;
use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Team {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TeamSummary {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
    pub member_count: i64,
    pub viewer_role: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TeamMemberDetail {
    pub account_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TeamsListResponse {
    pub teams: Vec<TeamSummary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TeamMembersResponse {
    pub members: Vec<TeamMemberDetail>,
}

#[derive(Debug, Serialize)]
struct CreateTeamInput<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    slug: Option<&'a str>,
}

#[derive(Debug, Default, Serialize)]
pub struct UpdateTeamInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
}

#[derive(Debug, Serialize)]
struct AddTeamMemberInput<'a> {
    account_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct PatchTeamMemberInput<'a> {
    role: &'a str,
}

async fn get_json<T: serde::de::DeserializeOwned>(url: Url, token: &str) -> Result<T> {
    let resp = reqwest::Client::new()
        .request(Method::GET, url)
        .bearer_auth(token)
        .send()
        .await?;
    let status = resp.status();
    if status.is_success() {
        Ok(resp.json().await?)
    } else {
        Err(translate_error(status, resp).await)
    }
}

async fn send_json<B: Serialize, T: serde::de::DeserializeOwned>(
    method: Method,
    url: Url,
    token: &str,
    body: &B,
) -> Result<T> {
    let resp = reqwest::Client::new()
        .request(method, url)
        .bearer_auth(token)
        .json(body)
        .send()
        .await?;
    let status = resp.status();
    if status.is_success() {
        Ok(resp.json().await?)
    } else {
        Err(translate_error(status, resp).await)
    }
}

async fn delete_no_body(url: Url, token: &str) -> Result<()> {
    let resp = reqwest::Client::new()
        .request(Method::DELETE, url)
        .bearer_auth(token)
        .send()
        .await?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(translate_error(status, resp).await)
    }
}

pub async fn teams_list(
    server_url: &Url,
    token: &str,
    org_id: Uuid,
) -> Result<TeamsListResponse> {
    get_json(
        server_url.join(&format!("v1/orgs/{org_id}/teams"))?,
        token,
    )
    .await
}

pub async fn team_create(
    server_url: &Url,
    token: &str,
    org_id: Uuid,
    name: &str,
    slug: Option<&str>,
) -> Result<Team> {
    send_json(
        Method::POST,
        server_url.join(&format!("v1/orgs/{org_id}/teams"))?,
        token,
        &CreateTeamInput { name, slug },
    )
    .await
}

pub async fn team_get(
    server_url: &Url,
    token: &str,
    org_id: Uuid,
    team_id: Uuid,
) -> Result<Team> {
    get_json(
        server_url.join(&format!("v1/orgs/{org_id}/teams/{team_id}"))?,
        token,
    )
    .await
}

pub async fn team_update(
    server_url: &Url,
    token: &str,
    org_id: Uuid,
    team_id: Uuid,
    input: &UpdateTeamInput,
) -> Result<Team> {
    send_json(
        Method::PATCH,
        server_url.join(&format!("v1/orgs/{org_id}/teams/{team_id}"))?,
        token,
        input,
    )
    .await
}

pub async fn team_delete(
    server_url: &Url,
    token: &str,
    org_id: Uuid,
    team_id: Uuid,
) -> Result<()> {
    delete_no_body(
        server_url.join(&format!("v1/orgs/{org_id}/teams/{team_id}"))?,
        token,
    )
    .await
}

pub async fn team_members(
    server_url: &Url,
    token: &str,
    org_id: Uuid,
    team_id: Uuid,
) -> Result<TeamMembersResponse> {
    get_json(
        server_url.join(&format!(
            "v1/orgs/{org_id}/teams/{team_id}/members"
        ))?,
        token,
    )
    .await
}

pub async fn team_member_add(
    server_url: &Url,
    token: &str,
    org_id: Uuid,
    team_id: Uuid,
    account_id: Uuid,
    role: Option<&str>,
) -> Result<TeamMemberDetail> {
    send_json(
        Method::POST,
        server_url.join(&format!(
            "v1/orgs/{org_id}/teams/{team_id}/members"
        ))?,
        token,
        &AddTeamMemberInput { account_id, role },
    )
    .await
}

pub async fn team_member_update(
    server_url: &Url,
    token: &str,
    org_id: Uuid,
    team_id: Uuid,
    account_id: Uuid,
    role: &str,
) -> Result<TeamMemberDetail> {
    send_json(
        Method::PATCH,
        server_url.join(&format!(
            "v1/orgs/{org_id}/teams/{team_id}/members/{account_id}"
        ))?,
        token,
        &PatchTeamMemberInput { role },
    )
    .await
}

pub async fn team_member_remove(
    server_url: &Url,
    token: &str,
    org_id: Uuid,
    team_id: Uuid,
    account_id: Uuid,
) -> Result<()> {
    delete_no_body(
        server_url.join(&format!(
            "v1/orgs/{org_id}/teams/{team_id}/members/{account_id}"
        ))?,
        token,
    )
    .await
}
