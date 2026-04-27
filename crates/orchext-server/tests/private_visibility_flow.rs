//! Phase 3 platform Slice 1 follow-up: visibility=private author-only
//! filtering. Pins the "My notes for [Org]" privacy guarantee — a
//! private doc written by alice in the org tenant must be invisible
//! to bob, even though bob is also a member of that org.
//!
//! Coverage:
//!   * list omits other members' private docs
//!   * read returns 404 to non-author
//!   * write to existing private doc by non-author returns 404
//!   * delete to existing private doc by non-author returns 404
//!   * doc-count omits other members' private docs
//!   * author can do all of the above on their own private doc
//!   * org-visible docs are still visible to all members
//!   * search results omit private docs from other members

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use orchext_server::{config::DeploymentMode, router, AppState};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

const MAX_BODY: usize = 1 << 20;

/// Alice's private doc to her org. Bob — also a member of the org —
/// must not see it via list, read, search, write, or delete.
#[sqlx::test(migrations = "./migrations")]
async fn private_doc_invisible_to_other_members(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::SelfHosted),
    );

    let (alice, alice_org_tenant_id, org_id) = bootstrap_owner(&app, "alice@example.com").await;
    let bob = approve_member(&app, &alice, &org_id, "bob@example.com").await;

    write_doc(
        &app,
        &alice,
        &alice_org_tenant_id,
        "alice-notes",
        ALICE_PRIVATE_DOC,
    )
    .await;

    // List as Bob — must be empty.
    let bob_list = list_docs(&app, &bob, &alice_org_tenant_id).await;
    assert!(
        bob_list["entries"].as_array().unwrap().is_empty(),
        "Bob's list of org docs must omit Alice's private doc"
    );

    // Read by id as Bob — must be 404.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/t/{alice_org_tenant_id}/vault/docs/alice-notes"
                ))
                .header("authorization", format!("Bearer {bob}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Write to alice-notes as Bob — must be 404 (not 403; we don't
    // want to leak existence).
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/v1/t/{alice_org_tenant_id}/vault/docs/alice-notes"
                ))
                .header("authorization", format!("Bearer {bob}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"source": ALICE_PRIVATE_DOC}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Delete as Bob — also 404.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/v1/t/{alice_org_tenant_id}/vault/docs/alice-notes"
                ))
                .header("authorization", format!("Bearer {bob}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn author_sees_their_own_private_doc(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::SelfHosted),
    );

    let (alice, alice_org_tenant_id, _org_id) =
        bootstrap_owner(&app, "alice@example.com").await;

    write_doc(
        &app,
        &alice,
        &alice_org_tenant_id,
        "alice-notes",
        ALICE_PRIVATE_DOC,
    )
    .await;

    // List as Alice — must include the doc.
    let list = list_docs(&app, &alice, &alice_org_tenant_id).await;
    let entries = list["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["doc_id"], "alice-notes");

    // Read as Alice — must succeed.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/t/{alice_org_tenant_id}/vault/docs/alice-notes"
                ))
                .header("authorization", format!("Bearer {alice}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn org_visibility_doc_visible_to_all_members(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::SelfHosted),
    );

    let (alice, alice_org_tenant_id, org_id) = bootstrap_owner(&app, "alice@example.com").await;
    let bob = approve_member(&app, &alice, &org_id, "bob@example.com").await;

    // Alice (owner) writes the org's mission with visibility=org.
    write_doc(
        &app,
        &alice,
        &alice_org_tenant_id,
        "mission",
        ORG_VISIBILITY_DOC,
    )
    .await;

    // Bob lists — must see it.
    let list = list_docs(&app, &bob, &alice_org_tenant_id).await;
    let entries = list["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["doc_id"], "mission");

    // Bob reads — must succeed.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/t/{alice_org_tenant_id}/vault/docs/mission"))
                .header("authorization", format!("Bearer {bob}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn doc_count_filters_other_authors_private_docs(db: PgPool) {
    let app = router(
        AppState::new(db)
            .with_rate_limit_auth(false)
            .with_deployment_mode(DeploymentMode::SelfHosted),
    );

    let (alice, alice_org_tenant_id, org_id) = bootstrap_owner(&app, "alice@example.com").await;
    let bob = approve_member(&app, &alice, &org_id, "bob@example.com").await;

    // Alice writes one private + one org-visible doc.
    write_doc(
        &app,
        &alice,
        &alice_org_tenant_id,
        "alice-notes",
        ALICE_PRIVATE_DOC,
    )
    .await;
    write_doc(
        &app,
        &alice,
        &alice_org_tenant_id,
        "mission",
        ORG_VISIBILITY_DOC,
    )
    .await;

    // Alice's doc-count includes both.
    let alice_count = doc_count(&app, &alice, &alice_org_tenant_id).await;
    assert_eq!(alice_count, 2);

    // Bob's doc-count includes only the org-visibility doc.
    let bob_count = doc_count(&app, &bob, &alice_org_tenant_id).await;
    assert_eq!(bob_count, 1);
}

// ---------- helpers ----------

/// Sign up `email` and return (bearer, org_tenant_id, org_id).
/// Uses self-hosted mode, so the first signup auto-bootstraps the
/// singleton org with this account as owner.
async fn bootstrap_owner(
    app: &axum::Router,
    email: &str,
) -> (String, String, String) {
    let bearer = signup_and_bearer(app, email).await;
    let orgs = json_get(app, &bearer, "/v1/orgs").await;
    let m = &orgs["memberships"][0];
    let tenant_id = m["tenant_id"].as_str().unwrap().to_string();
    let org_id = m["org_id"].as_str().unwrap().to_string();
    (bearer, tenant_id, org_id)
}

async fn approve_member(
    app: &axum::Router,
    owner_bearer: &str,
    org_id: &str,
    email: &str,
) -> String {
    let bearer = signup_and_bearer(app, email).await;
    let me = json_get(app, &bearer, "/v1/auth/me").await;
    let account_id = me["account"]["id"].as_str().unwrap().to_string();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/orgs/{org_id}/pending/{account_id}/approve"
                ))
                .header("authorization", format!("Bearer {owner_bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"role": "member"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "approve failed for {email}");
    bearer
}

async fn signup_and_bearer(app: &axum::Router, email: &str) -> String {
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
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = read_json(resp.into_body()).await;
    body["session"]["secret"].as_str().unwrap().to_string()
}

async fn json_get(app: &axum::Router, bearer: &str, uri: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    read_json(resp.into_body()).await
}

async fn list_docs(app: &axum::Router, bearer: &str, tenant_id: &str) -> Value {
    json_get(app, bearer, &format!("/v1/t/{tenant_id}/vault/docs")).await
}

async fn doc_count(app: &axum::Router, bearer: &str, tenant_id: &str) -> i64 {
    let body = json_get(
        app,
        bearer,
        &format!("/v1/t/{tenant_id}/vault/doc-count"),
    )
    .await;
    body["count"].as_i64().unwrap()
}

async fn write_doc(
    app: &axum::Router,
    bearer: &str,
    tenant_id: &str,
    doc_id: &str,
    source: &str,
) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/t/{tenant_id}/vault/docs/{doc_id}"))
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"source": source}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = read_json(resp.into_body()).await;
    assert!(status.is_success(), "write_doc {doc_id} got {status}: {body}");
}

async fn read_json(body: Body) -> Value {
    let bytes = to_bytes(body, MAX_BODY).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

const ALICE_PRIVATE_DOC: &str = "---\n\
id: alice-notes\n\
type: memories\n\
visibility: private\n\
tags: []\n\
links: []\n\
updated: 2026-04-27\n\
---\n\
# Alice's private notes\n\
\n\
Things only I should see.\n";

const ORG_VISIBILITY_DOC: &str = "---\n\
id: mission\n\
type: org\n\
visibility: org\n\
tags: []\n\
links: []\n\
updated: 2026-04-27\n\
---\n\
# Mission\n\
\n\
Shared with all members.\n";
