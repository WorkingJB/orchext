//! Organizations: metadata layer above the storage tenant
//! (Phase 3 platform Slice 1, D10 revised).
//!
//! Holds two roles:
//!   1. **HTTP routes** under `/v1/orgs/*` for read/update/create.
//!   2. **Signup helpers** invoked from `accounts::signup` —
//!      `bootstrap_self_hosted` and `bootstrap_saas` — that decide
//!      whether a fresh signup becomes the first owner of a new org
//!      or lands in `pending_signups` for an existing one.
//!
//! v1 enforces a 1:1 mapping between `organizations` and `kind='org'`
//! tenants via the UNIQUE FK. The schema leaves room to decouple
//! later if a customer asks (D10 revised).

use crate::{
    accounts::Account, error::ApiError, sessions::SessionContext, AppState,
};
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
pub struct Organization {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub logo_url: Option<String>,
    pub allowed_domains: serde_json::Value,
    pub settings: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct OrgMembership {
    pub org_id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub logo_url: Option<String>,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PendingSignup {
    pub id: Uuid,
    pub org_id: Uuid,
    pub org_name: String,
    pub requested_role: String,
    pub status: String,
    pub requested_at: DateTime<Utc>,
}

/// Outcome of a signup. Returned by `accounts::signup` so callers can
/// log + (eventually) shape responses based on whether the new account
/// has an immediate org membership or is awaiting approval.
#[derive(Debug, Clone)]
pub enum SignupOutcome {
    /// New org was created and the signup became its `owner`.
    BootstrappedOrg { org_id: Uuid, tenant_id: Uuid },
    /// Account exists but has no org membership yet — pending row
    /// landed for an admin to approve.
    AwaitingApproval { org_id: Uuid, pending_id: Uuid },
}

// ---------- signup helpers (called from accounts::signup) ----------

/// Self-hosted: first signup → owner of new singleton org.
/// Subsequent signups → pending for the existing singleton.
///
/// Race note: two concurrent first-signups can each see "no org" and
/// both create one. The result is a server with two orgs and two
/// owners — recoverable via admin cleanup. Acceptable for v1; tighten
/// with an advisory lock if it ever bites in practice.
pub async fn bootstrap_self_hosted(
    tx: &mut Transaction<'_, Postgres>,
    account: &Account,
) -> Result<SignupOutcome, ApiError> {
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM organizations ORDER BY created_at ASC LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await?;

    match existing {
        None => {
            let (org_id, tenant_id) =
                create_org_and_membership(tx, account, "Organization", &[]).await?;
            Ok(SignupOutcome::BootstrappedOrg { org_id, tenant_id })
        }
        Some((org_id,)) => {
            let pending_id = create_pending(tx, account.id, org_id).await?;
            Ok(SignupOutcome::AwaitingApproval { org_id, pending_id })
        }
    }
}

/// SaaS: signup with email domain matching some org's `allowed_domains`
/// → pending for that org. Otherwise → owner of a new org claiming
/// the email domain.
///
/// D17e (deferred): once email verification ships, the matching-domain
/// path will skip pending and create membership directly. Until then,
/// matching-domain still pends so a `mallory@acme.com` who never had
/// access to acme.com can't auto-land inside Acme's org.
pub async fn bootstrap_saas(
    tx: &mut Transaction<'_, Postgres>,
    account: &Account,
    email: &str,
) -> Result<SignupOutcome, ApiError> {
    let domain = email.split('@').nth(1).unwrap_or("").to_lowercase();
    if domain.is_empty() {
        return Err(ApiError::InvalidArgument("email must contain '@'".into()));
    }

    let matching: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT id FROM organizations
        WHERE allowed_domains @> to_jsonb($1::text)
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )
    .bind(&domain)
    .fetch_optional(&mut **tx)
    .await?;

    match matching {
        Some((org_id,)) => {
            let pending_id = create_pending(tx, account.id, org_id).await?;
            Ok(SignupOutcome::AwaitingApproval { org_id, pending_id })
        }
        None => {
            let (org_id, tenant_id) = create_org_and_membership(
                tx,
                account,
                &default_org_name(&domain),
                &[domain],
            )
            .await?;
            Ok(SignupOutcome::BootstrappedOrg { org_id, tenant_id })
        }
    }
}

async fn create_org_and_membership(
    tx: &mut Transaction<'_, Postgres>,
    owner: &Account,
    name: &str,
    allowed_domains: &[String],
) -> Result<(Uuid, Uuid), ApiError> {
    let tenant_row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO tenants (name, kind)
        VALUES ($1, 'org')
        RETURNING id
        "#,
    )
    .bind(name)
    .fetch_one(&mut **tx)
    .await?;

    let allowed_domains_json = serde_json::to_value(allowed_domains)
        .unwrap_or(serde_json::Value::Array(vec![]));

    let org_row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO organizations (tenant_id, name, allowed_domains)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
    )
    .bind(tenant_row.0)
    .bind(name)
    .bind(allowed_domains_json)
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO memberships (tenant_id, account_id, role)
        VALUES ($1, $2, 'owner')
        "#,
    )
    .bind(tenant_row.0)
    .bind(owner.id)
    .execute(&mut **tx)
    .await?;

    Ok((org_row.0, tenant_row.0))
}

async fn create_pending(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    org_id: Uuid,
) -> Result<Uuid, ApiError> {
    let row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO pending_signups (account_id, org_id, requested_role)
        VALUES ($1, $2, 'member')
        RETURNING id
        "#,
    )
    .bind(account_id)
    .bind(org_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.0)
}

fn default_org_name(domain: &str) -> String {
    // "acme.com" → "Acme". Strip TLD and title-case the head.
    let head = domain.split('.').next().unwrap_or(domain);
    if head.is_empty() {
        return "Organization".into();
    }
    let mut chars = head.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Organization".into(),
    }
}

// ---------- HTTP routes ----------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/orgs", get(list_orgs).post(create_org))
        .route("/orgs/:org_id", get(get_org).patch(update_org))
}

#[derive(Debug, Serialize)]
struct OrgsListResponse {
    memberships: Vec<OrgMembership>,
    pending: Vec<PendingSignup>,
}

async fn list_orgs(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
) -> Result<Json<OrgsListResponse>, ApiError> {
    let memberships: Vec<OrgMembership> = sqlx::query_as(
        r#"
        SELECT
            o.id          AS org_id,
            o.tenant_id   AS tenant_id,
            o.name        AS name,
            o.logo_url    AS logo_url,
            m.role        AS role,
            m.created_at  AS joined_at
        FROM memberships m
        JOIN tenants t      ON t.id = m.tenant_id AND t.kind = 'org'
        JOIN organizations o ON o.tenant_id = t.id
        WHERE m.account_id = $1
        ORDER BY m.created_at ASC
        "#,
    )
    .bind(ctx.account_id)
    .fetch_all(&state.db)
    .await?;

    let pending: Vec<PendingSignup> = sqlx::query_as(
        r#"
        SELECT
            p.id             AS id,
            p.org_id         AS org_id,
            o.name           AS org_name,
            p.requested_role AS requested_role,
            p.status         AS status,
            p.requested_at   AS requested_at
        FROM pending_signups p
        JOIN organizations o ON o.id = p.org_id
        WHERE p.account_id = $1 AND p.status = 'pending'
        ORDER BY p.requested_at ASC
        "#,
    )
    .bind(ctx.account_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(OrgsListResponse {
        memberships,
        pending,
    }))
}

async fn get_org(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Organization>, ApiError> {
    require_membership(&state.db, ctx.account_id, org_id).await?;
    let org = fetch_org(&state.db, org_id).await?;
    Ok(Json(org))
}

#[derive(Debug, Deserialize)]
struct UpdateOrgInput {
    name: Option<String>,
    logo_url: Option<String>,
    allowed_domains: Option<Vec<String>>,
    settings: Option<serde_json::Value>,
}

async fn update_org(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path(org_id): Path<Uuid>,
    Json(input): Json<UpdateOrgInput>,
) -> Result<Json<Organization>, ApiError> {
    let role = require_membership(&state.db, ctx.account_id, org_id).await?;
    if !matches!(role.as_str(), "owner" | "admin") {
        return Err(ApiError::Forbidden);
    }

    let mut tx = state.db.begin().await?;
    if let Some(name) = input.name.as_deref() {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApiError::InvalidArgument("name must not be empty".into()));
        }
        sqlx::query("UPDATE organizations SET name = $1 WHERE id = $2")
            .bind(trimmed)
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
        // Mirror the org name into the underlying tenant row so the
        // existing `/v1/tenants` listing stays human-readable.
        sqlx::query(
            r#"
            UPDATE tenants
            SET name = $1
            WHERE id = (SELECT tenant_id FROM organizations WHERE id = $2)
            "#,
        )
        .bind(trimmed)
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(logo_url) = input.logo_url.as_ref() {
        sqlx::query("UPDATE organizations SET logo_url = $1 WHERE id = $2")
            .bind(if logo_url.trim().is_empty() {
                None
            } else {
                Some(logo_url.as_str())
            })
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(domains) = input.allowed_domains.as_ref() {
        let normalized: Vec<String> = domains
            .iter()
            .map(|d| d.trim().to_lowercase())
            .filter(|d| !d.is_empty())
            .collect();
        sqlx::query("UPDATE organizations SET allowed_domains = $1 WHERE id = $2")
            .bind(serde_json::to_value(&normalized).unwrap())
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(settings) = input.settings.as_ref() {
        sqlx::query("UPDATE organizations SET settings = $1 WHERE id = $2")
            .bind(settings)
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    Ok(Json(fetch_org(&state.db, org_id).await?))
}

#[derive(Debug, Deserialize)]
struct CreateOrgInput {
    name: String,
}

async fn create_org(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Json(input): Json<CreateOrgInput>,
) -> Result<Json<Organization>, ApiError> {
    let trimmed = input.name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::InvalidArgument("name must not be empty".into()));
    }

    let mut tx = state.db.begin().await?;
    let account = crate::accounts::by_id_in(&mut tx, ctx.account_id).await?;
    let (org_id, _tenant_id) =
        create_org_and_membership(&mut tx, &account, trimmed, &[]).await?;
    tx.commit().await?;

    let org = fetch_org(&state.db, org_id).await?;
    Ok(Json(org))
}

// ---------- helpers ----------

async fn require_membership(
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
    row.map(|(role,)| role).ok_or(ApiError::NotFound)
}

async fn fetch_org(
    db: &sqlx::PgPool,
    org_id: Uuid,
) -> Result<Organization, ApiError> {
    let org: Option<Organization> = sqlx::query_as(
        r#"
        SELECT id, tenant_id, name, logo_url, allowed_domains, settings, created_at
        FROM organizations
        WHERE id = $1
        "#,
    )
    .bind(org_id)
    .fetch_optional(db)
    .await?;
    org.ok_or(ApiError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_org_name_strips_tld_and_titlecases() {
        assert_eq!(default_org_name("acme.com"), "Acme");
        assert_eq!(default_org_name("foo-bar.io"), "Foo-bar");
        assert_eq!(default_org_name("a.b.c"), "A");
        assert_eq!(default_org_name(""), "Organization");
    }
}
