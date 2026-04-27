//! Teams: logical groupings inside an org tenant (Phase 3 platform
//! Slice 2, D17c).
//!
//! Teams are *not* separate vaults. They share the org tenant's audit
//! chain, session key, and encryption material; the access boundary
//! lives at the visibility-filter layer in `documents.rs`. A doc with
//! `visibility = 'team'` carries a non-null `team_id` and is readable
//! only by accounts with a row in `team_memberships` for that team
//! (org admins/owners pass too — see D11).
//!
//! Routes mount under `/v1/orgs/:org_id/teams[/...]` alongside the
//! existing org admin surface; the team admin gate combines org-admin
//! and team-manager into a single `require_team_admin` helper.
//!
//! Cuts in v1: no team-manager rename rights for team members below
//! the manager role; no per-team audit feed; no cryptographic
//! per-team separation (Phase 3e.3 if a customer asks).

use crate::{error::ApiError, sessions::SessionContext, AppState};
use axum::{
    extract::{Path, State},
    routing::get,
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

// ---------- types ----------

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Team {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

/// Enriched team summary returned by the list endpoint: includes
/// whether the caller is a member and (if so) their team-level role.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TeamSummary {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
    pub member_count: i64,
    pub viewer_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TeamMemberDetail {
    pub account_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

// ---------- HTTP routes ----------

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/orgs/:org_id/teams",
            get(list_teams).post(create_team),
        )
        .route(
            "/orgs/:org_id/teams/:team_id",
            get(get_team)
                .patch(update_team)
                .delete(delete_team),
        )
        .route(
            "/orgs/:org_id/teams/:team_id/members",
            get(list_team_members).post(add_team_member),
        )
        .route(
            "/orgs/:org_id/teams/:team_id/members/:account_id",
            axum::routing::patch(patch_team_member).delete(remove_team_member),
        )
}

#[derive(Debug, Serialize)]
struct TeamsListResponse {
    teams: Vec<TeamSummary>,
}

async fn list_teams(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path(org_id): Path<Uuid>,
) -> Result<Json<TeamsListResponse>, ApiError> {
    require_org_membership(&state.db, ctx.account_id, org_id).await?;
    let teams: Vec<TeamSummary> = sqlx::query_as(
        r#"
        SELECT
            t.id          AS id,
            t.org_id      AS org_id,
            t.name        AS name,
            t.slug        AS slug,
            t.created_at  AS created_at,
            COALESCE((
                SELECT COUNT(*) FROM team_memberships tm
                WHERE tm.team_id = t.id
            ), 0) AS member_count,
            (
                SELECT tm.role FROM team_memberships tm
                WHERE tm.team_id = t.id AND tm.account_id = $2
            ) AS viewer_role
        FROM teams t
        WHERE t.org_id = $1
        ORDER BY t.created_at ASC
        "#,
    )
    .bind(org_id)
    .bind(ctx.account_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(TeamsListResponse { teams }))
}

#[derive(Debug, Deserialize)]
struct CreateTeamInput {
    name: String,
    /// Optional slug override. Auto-derived from `name` when omitted.
    /// Slug uniqueness is per-org (the UNIQUE index on (org_id, slug)
    /// surfaces collisions as 409).
    #[serde(default)]
    slug: Option<String>,
}

async fn create_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path(org_id): Path<Uuid>,
    Json(input): Json<CreateTeamInput>,
) -> Result<Json<Team>, ApiError> {
    require_org_admin(&state.db, ctx.account_id, org_id).await?;

    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::InvalidArgument("name must not be empty".into()));
    }
    let slug = match input.slug.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => normalize_slug(s),
        None => derive_slug(name),
    };
    if slug.is_empty() {
        return Err(ApiError::InvalidArgument(
            "slug must contain at least one alphanumeric character".into(),
        ));
    }

    let row: Result<Team, sqlx::Error> = sqlx::query_as(
        r#"
        INSERT INTO teams (org_id, name, slug)
        VALUES ($1, $2, $3)
        RETURNING id, org_id, name, slug, created_at
        "#,
    )
    .bind(org_id)
    .bind(name)
    .bind(&slug)
    .fetch_one(&state.db)
    .await;

    match row {
        Ok(team) => Ok(Json(team)),
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            Err(ApiError::Conflict("team slug already in use"))
        }
        Err(e) => Err(ApiError::Internal(Box::new(e))),
    }
}

async fn get_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path((org_id, team_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Team>, ApiError> {
    require_org_membership(&state.db, ctx.account_id, org_id).await?;
    let team = fetch_team(&state.db, org_id, team_id).await?;
    Ok(Json(team))
}

#[derive(Debug, Deserialize)]
struct UpdateTeamInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    slug: Option<String>,
}

async fn update_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path((org_id, team_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<UpdateTeamInput>,
) -> Result<Json<Team>, ApiError> {
    require_team_admin(&state.db, ctx.account_id, org_id, team_id).await?;

    let mut tx = state.db.begin().await?;
    if let Some(name) = input.name.as_deref() {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApiError::InvalidArgument("name must not be empty".into()));
        }
        sqlx::query("UPDATE teams SET name = $1 WHERE id = $2 AND org_id = $3")
            .bind(trimmed)
            .bind(team_id)
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(slug) = input.slug.as_deref() {
        let normalized = normalize_slug(slug);
        if normalized.is_empty() {
            return Err(ApiError::InvalidArgument(
                "slug must contain at least one alphanumeric character".into(),
            ));
        }
        let result = sqlx::query(
            "UPDATE teams SET slug = $1 WHERE id = $2 AND org_id = $3",
        )
        .bind(&normalized)
        .bind(team_id)
        .bind(org_id)
        .execute(&mut *tx)
        .await;
        if let Err(sqlx::Error::Database(db)) = &result {
            if db.code().as_deref() == Some("23505") {
                return Err(ApiError::Conflict("team slug already in use"));
            }
        }
        result.map_err(|e| ApiError::Internal(Box::new(e)))?;
    }
    tx.commit().await?;

    let team = fetch_team(&state.db, org_id, team_id).await?;
    Ok(Json(team))
}

async fn delete_team(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path((org_id, team_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_org_admin(&state.db, ctx.account_id, org_id).await?;
    let result = sqlx::query("DELETE FROM teams WHERE id = $1 AND org_id = $2")
        .bind(team_id)
        .bind(org_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------- members ----------

#[derive(Debug, Serialize)]
struct TeamMembersResponse {
    members: Vec<TeamMemberDetail>,
}

async fn list_team_members(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path((org_id, team_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TeamMembersResponse>, ApiError> {
    require_org_membership(&state.db, ctx.account_id, org_id).await?;
    // Existence check: the team must belong to this org. fetch_team
    // surfaces 404 if it doesn't, which doubles as enumeration
    // resistance.
    fetch_team(&state.db, org_id, team_id).await?;

    let members: Vec<TeamMemberDetail> = sqlx::query_as(
        r#"
        SELECT
            a.id           AS account_id,
            a.email        AS email,
            a.display_name AS display_name,
            tm.role        AS role,
            tm.created_at  AS joined_at
        FROM team_memberships tm
        JOIN accounts a ON a.id = tm.account_id
        WHERE tm.team_id = $1
        ORDER BY tm.created_at ASC
        "#,
    )
    .bind(team_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(TeamMembersResponse { members }))
}

#[derive(Debug, Deserialize)]
struct AddTeamMemberInput {
    account_id: Uuid,
    #[serde(default)]
    role: Option<String>,
}

async fn add_team_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path((org_id, team_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<AddTeamMemberInput>,
) -> Result<Json<TeamMemberDetail>, ApiError> {
    require_team_admin(&state.db, ctx.account_id, org_id, team_id).await?;
    fetch_team(&state.db, org_id, team_id).await?;

    let role = input
        .role
        .unwrap_or_else(|| "member".into())
        .to_lowercase();
    if !matches!(role.as_str(), "manager" | "member") {
        return Err(ApiError::InvalidArgument(
            "role must be one of manager, member".into(),
        ));
    }

    // Target must already be an org member — adding to a team should
    // not back-door org membership.
    let org_member: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT m.role
        FROM memberships m
        JOIN organizations o ON o.tenant_id = m.tenant_id
        WHERE m.account_id = $1 AND o.id = $2
        "#,
    )
    .bind(input.account_id)
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    if org_member.is_none() {
        return Err(ApiError::Conflict(
            "account is not a member of this org",
        ));
    }

    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO team_memberships (team_id, account_id, role)
        VALUES ($1, $2, $3)
        ON CONFLICT (team_id, account_id) DO UPDATE SET role = EXCLUDED.role
        "#,
    )
    .bind(team_id)
    .bind(input.account_id)
    .bind(&role)
    .execute(&mut *tx)
    .await?;

    let member = fetch_team_member(&mut tx, team_id, input.account_id).await?;
    tx.commit().await?;
    Ok(Json(member))
}

#[derive(Debug, Deserialize)]
struct PatchTeamMemberInput {
    role: String,
}

async fn patch_team_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path((org_id, team_id, target_account_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<PatchTeamMemberInput>,
) -> Result<Json<TeamMemberDetail>, ApiError> {
    require_team_admin(&state.db, ctx.account_id, org_id, team_id).await?;
    let new_role = input.role.trim().to_lowercase();
    if !matches!(new_role.as_str(), "manager" | "member") {
        return Err(ApiError::InvalidArgument(
            "role must be one of manager, member".into(),
        ));
    }

    let mut tx = state.db.begin().await?;
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM team_memberships WHERE team_id = $1 AND account_id = $2 FOR UPDATE",
    )
    .bind(team_id)
    .bind(target_account_id)
    .fetch_optional(&mut *tx)
    .await?;
    if existing.is_none() {
        return Err(ApiError::NotFound);
    }

    sqlx::query(
        "UPDATE team_memberships SET role = $1 WHERE team_id = $2 AND account_id = $3",
    )
    .bind(&new_role)
    .bind(team_id)
    .bind(target_account_id)
    .execute(&mut *tx)
    .await?;
    let member = fetch_team_member(&mut tx, team_id, target_account_id).await?;
    tx.commit().await?;
    Ok(Json(member))
}

async fn remove_team_member(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path((org_id, team_id, target_account_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_team_admin(&state.db, ctx.account_id, org_id, team_id).await?;
    let result = sqlx::query(
        "DELETE FROM team_memberships WHERE team_id = $1 AND account_id = $2",
    )
    .bind(team_id)
    .bind(target_account_id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------- helpers ----------

/// Ensures the caller has any membership in the org. Returns the org
/// role for callers that want to branch on it; team-scoped handlers
/// pass through to `require_team_admin` instead.
async fn require_org_membership(
    db: &sqlx::PgPool,
    account_id: Uuid,
    org_id: Uuid,
) -> Result<String, ApiError> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT m.role
        FROM memberships m
        JOIN organizations o ON o.tenant_id = m.tenant_id
        WHERE m.account_id = $1 AND o.id = $2
        "#,
    )
    .bind(account_id)
    .bind(org_id)
    .fetch_optional(db)
    .await?;
    row.map(|(r,)| r).ok_or(ApiError::NotFound)
}

async fn require_org_admin(
    db: &sqlx::PgPool,
    account_id: Uuid,
    org_id: Uuid,
) -> Result<String, ApiError> {
    let role = require_org_membership(db, account_id, org_id).await?;
    if !matches!(role.as_str(), "owner" | "admin") {
        return Err(ApiError::Forbidden);
    }
    Ok(role)
}

/// Combined gate: org admin/owner OR manager of *this* team. Returns
/// `("org", role)` or `("team", role)` so callers can branch if they
/// ever need to know which path admitted them; today no caller does,
/// so the returned tuple is mostly informational.
async fn require_team_admin(
    db: &sqlx::PgPool,
    account_id: Uuid,
    org_id: Uuid,
    team_id: Uuid,
) -> Result<(&'static str, String), ApiError> {
    let org_role = require_org_membership(db, account_id, org_id).await?;
    if matches!(org_role.as_str(), "owner" | "admin") {
        return Ok(("org", org_role));
    }
    let team_row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT tm.role
        FROM team_memberships tm
        JOIN teams t ON t.id = tm.team_id
        WHERE tm.account_id = $1 AND tm.team_id = $2 AND t.org_id = $3
        "#,
    )
    .bind(account_id)
    .bind(team_id)
    .bind(org_id)
    .fetch_optional(db)
    .await?;
    match team_row {
        Some((role,)) if role == "manager" => Ok(("team", role)),
        _ => Err(ApiError::Forbidden),
    }
}

async fn fetch_team(
    db: &sqlx::PgPool,
    org_id: Uuid,
    team_id: Uuid,
) -> Result<Team, ApiError> {
    let team: Option<Team> = sqlx::query_as(
        r#"
        SELECT id, org_id, name, slug, created_at
        FROM teams
        WHERE id = $1 AND org_id = $2
        "#,
    )
    .bind(team_id)
    .bind(org_id)
    .fetch_optional(db)
    .await?;
    team.ok_or(ApiError::NotFound)
}

async fn fetch_team_member(
    tx: &mut Transaction<'_, Postgres>,
    team_id: Uuid,
    account_id: Uuid,
) -> Result<TeamMemberDetail, ApiError> {
    let row: Option<TeamMemberDetail> = sqlx::query_as(
        r#"
        SELECT
            a.id           AS account_id,
            a.email        AS email,
            a.display_name AS display_name,
            tm.role        AS role,
            tm.created_at  AS joined_at
        FROM team_memberships tm
        JOIN accounts a ON a.id = tm.account_id
        WHERE tm.team_id = $1 AND tm.account_id = $2
        "#,
    )
    .bind(team_id)
    .bind(account_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.ok_or(ApiError::NotFound)
}

/// Slugify a free-text team name: lowercase, ASCII alphanumerics +
/// dashes only, collapse runs of separators, strip leading/trailing
/// dashes. Matches the regex "first char ascii lowercase, rest
/// [a-z0-9-]" loosely enough that "Marketing Ops" → "marketing-ops".
fn derive_slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = true; // avoid leading dash
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Normalize a slug provided directly by the caller. Same allowed
/// alphabet as `derive_slug` but applied as a validator-cum-cleaner so
/// callers can't smuggle whitespace or uppercase past the UNIQUE
/// index.
fn normalize_slug(s: &str) -> String {
    derive_slug(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_lowercases_and_dashifies() {
        assert_eq!(derive_slug("Marketing Ops"), "marketing-ops");
        assert_eq!(derive_slug("  Field // Sales  "), "field-sales");
        assert_eq!(derive_slug("Dev_Team_2"), "dev-team-2");
        assert_eq!(derive_slug("---"), "");
    }
}
