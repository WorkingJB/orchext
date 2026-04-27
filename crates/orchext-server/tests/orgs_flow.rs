//! End-to-end signup → org-assignment flow tests.
//!
//! Covers Phase 3 platform Slice 1: D17d (approval queue gates every
//! signup) + the deployment-mode rules in `accounts::signup` for both
//! `self_hosted` and `saas` modes. Hits real Postgres via `sqlx::test`.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use orchext_server::{config::DeploymentMode, router, AppState};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

const MAX_BODY: usize = 1 << 20;

// ---------- self-hosted ----------

#[sqlx::test(migrations = "./migrations")]
async fn self_hosted_first_signup_owns_singleton_org(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::SelfHosted),
    );

    let secret = signup_and_get_bearer(&app, "first@example.com").await;

    let orgs = get_orgs(&app, &secret).await;
    let memberships = orgs["memberships"].as_array().unwrap();
    let pending = orgs["pending"].as_array().unwrap();

    assert_eq!(memberships.len(), 1, "first signup gets exactly one org membership");
    assert_eq!(memberships[0]["role"], "owner");
    assert!(pending.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn self_hosted_second_signup_lands_in_pending(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::SelfHosted),
    );

    let _ = signup_and_get_bearer(&app, "owner@example.com").await;
    let second_secret = signup_and_get_bearer(&app, "second@example.com").await;

    let orgs = get_orgs(&app, &second_secret).await;
    let memberships = orgs["memberships"].as_array().unwrap();
    let pending = orgs["pending"].as_array().unwrap();

    assert!(memberships.is_empty(), "second signup has no org membership yet");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["status"], "pending");
    assert_eq!(pending[0]["requested_role"], "member");
}

// ---------- SaaS ----------

#[sqlx::test(migrations = "./migrations")]
async fn saas_first_signup_per_domain_creates_org(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::Saas),
    );

    let secret = signup_and_get_bearer(&app, "alice@acme.com").await;

    let orgs = get_orgs(&app, &secret).await;
    let memberships = orgs["memberships"].as_array().unwrap();
    assert_eq!(memberships.len(), 1);
    assert_eq!(memberships[0]["role"], "owner");
    assert_eq!(memberships[0]["name"], "Acme");

    // Confirm allowed_domains was claimed by reading the org metadata.
    let org_id = memberships[0]["org_id"].as_str().unwrap();
    let detail = get_org(&app, &secret, org_id).await;
    let domains = detail["allowed_domains"].as_array().unwrap();
    assert_eq!(domains.len(), 1);
    assert_eq!(domains[0], "acme.com");
}

#[sqlx::test(migrations = "./migrations")]
async fn saas_matching_domain_lands_in_pending(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::Saas),
    );

    let _ = signup_and_get_bearer(&app, "alice@acme.com").await;
    let bob_secret = signup_and_get_bearer(&app, "bob@acme.com").await;

    let orgs = get_orgs(&app, &bob_secret).await;
    let memberships = orgs["memberships"].as_array().unwrap();
    let pending = orgs["pending"].as_array().unwrap();
    assert!(memberships.is_empty(), "matching-domain signup is pending, not auto-joined");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["status"], "pending");
}

#[sqlx::test(migrations = "./migrations")]
async fn saas_different_domain_creates_separate_org(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::Saas),
    );

    let alice_secret = signup_and_get_bearer(&app, "alice@acme.com").await;
    let chris_secret = signup_and_get_bearer(&app, "chris@beta.com").await;

    let alice_orgs = get_orgs(&app, &alice_secret).await;
    let chris_orgs = get_orgs(&app, &chris_secret).await;

    let alice_org_id = alice_orgs["memberships"][0]["org_id"].as_str().unwrap();
    let chris_org_id = chris_orgs["memberships"][0]["org_id"].as_str().unwrap();

    assert_ne!(alice_org_id, chris_org_id, "different domains land in different orgs");
    assert_eq!(chris_orgs["memberships"][0]["name"], "Beta");
}

// ---------- org metadata API ----------

#[sqlx::test(migrations = "./migrations")]
async fn get_org_for_non_member_is_not_found(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::Saas),
    );

    let alice = signup_and_get_bearer(&app, "alice@acme.com").await;
    let chris = signup_and_get_bearer(&app, "chris@beta.com").await;

    let alice_orgs = get_orgs(&app, &alice).await;
    let alice_org_id = alice_orgs["memberships"][0]["org_id"].as_str().unwrap().to_string();

    // Chris is not a member of Acme — should 404, not leak existence.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/orgs/{alice_org_id}"))
                .header("authorization", format!("Bearer {chris}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_org_member_forbidden(db: PgPool) {
    // Member-role patch is rejected with 403. We can't easily create a
    // non-owner member yet (Slice 1 ships the approval queue without
    // auto-joining the test account into an existing org), so this
    // exercises the role-gating branch using a self-hosted second
    // signup that has no membership at all (NotFound) plus a contrived
    // scenario via direct DB insert.
    let pool = db.clone();
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::SelfHosted),
    );

    let owner = signup_and_get_bearer(&app, "owner@example.com").await;
    let owner_orgs = get_orgs(&app, &owner).await;
    let org_id = owner_orgs["memberships"][0]["org_id"].as_str().unwrap().to_string();
    let tenant_id = owner_orgs["memberships"][0]["tenant_id"].as_str().unwrap().to_string();

    // Bring a second account into the same org as a `member` by direct
    // INSERT — covers the role gate without needing the approval-queue
    // endpoint that ships in commit 3.
    let second_secret = signup_and_get_bearer(&app, "member@example.com").await;
    let second_account_id: (uuid::Uuid,) = sqlx::query_as(
        "SELECT account_id FROM sessions WHERE token_prefix = $1",
    )
    .bind(&second_secret[.."ocx_".len() + 8])
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships (tenant_id, account_id, role) VALUES ($1, $2, 'member')",
    )
    .bind(uuid::Uuid::parse_str(&tenant_id).unwrap())
    .bind(second_account_id.0)
    .execute(&pool)
    .await
    .unwrap();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/orgs/{org_id}"))
                .header("authorization", format!("Bearer {second_secret}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"name": "Hijacked"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn patch_org_owner_updates_name_and_mirrors_to_tenant(db: PgPool) {
    let pool = db.clone();
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::SelfHosted),
    );

    let owner = signup_and_get_bearer(&app, "owner@example.com").await;
    let orgs = get_orgs(&app, &owner).await;
    let org_id = orgs["memberships"][0]["org_id"].as_str().unwrap().to_string();
    let tenant_id = orgs["memberships"][0]["tenant_id"].as_str().unwrap().to_string();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/orgs/{org_id}"))
                .header("authorization", format!("Bearer {owner}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"name": "Renamed Co", "logo_url": "https://example.com/logo.png"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = read_json(resp.into_body()).await;
    assert_eq!(body["name"], "Renamed Co");
    assert_eq!(body["logo_url"], "https://example.com/logo.png");

    // Tenant name should mirror so the existing /v1/tenants listing
    // stays human-readable.
    let tenant_name: (String,) =
        sqlx::query_as("SELECT name FROM tenants WHERE id = $1")
            .bind(uuid::Uuid::parse_str(&tenant_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(tenant_name.0, "Renamed Co");
}

#[sqlx::test(migrations = "./migrations")]
async fn post_org_creates_out_of_band(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::SelfHosted),
    );

    let owner = signup_and_get_bearer(&app, "owner@example.com").await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/orgs")
                .header("authorization", format!("Bearer {owner}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"name": "Side Project"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = read_json(resp.into_body()).await;
    assert_eq!(body["name"], "Side Project");

    // Caller now belongs to two orgs (the bootstrap org + the new one).
    let orgs = get_orgs(&app, &owner).await;
    assert_eq!(orgs["memberships"].as_array().unwrap().len(), 2);
}

// ---------- helpers ----------

async fn signup_and_get_bearer(app: &axum::Router, email: &str) -> String {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/native/signup")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "email": email,
                        "password": "correct horse battery staple",
                        "display_name": "User"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "signup failed for {email}");
    let body: Value = read_json(resp.into_body()).await;
    body["session"]["secret"].as_str().unwrap().to_string()
}

async fn get_orgs(app: &axum::Router, bearer: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/orgs")
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    read_json(resp.into_body()).await
}

async fn get_org(app: &axum::Router, bearer: &str, org_id: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/orgs/{org_id}"))
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    read_json(resp.into_body()).await
}

async fn read_json(body: Body) -> Value {
    let bytes = to_bytes(body, MAX_BODY).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
