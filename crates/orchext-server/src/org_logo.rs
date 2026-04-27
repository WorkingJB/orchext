//! Org logo upload + serve.
//!
//! Slice 1 had the admin paste an external HTTPS URL into
//! `organizations.logo_url`. In practice CDN expirations, mixed-content
//! warnings, and CORS blocked the image often enough to be the
//! complaint that opened Slice 2; we now host the bytes ourselves and
//! `logo_url` becomes a path back into this route.
//!
//! Storage lives in `org_logos` (one row per org, BYTEA + content type
//! + sha256). The GET path returns the bytes with an ETag pinned to
//! the sha256 so clients can cache aggressively. POST is admin/owner
//! gated and validates type + size + magic bytes; DELETE drops the
//! row and nulls `logo_url`.
//!
//! See migration `0011_org_logos.sql`.

use crate::{error::ApiError, sessions::SessionContext, AppState};
use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
    Extension, Json, Router,
};
use serde::Serialize;
use uuid::Uuid;

/// Hard cap on uploaded logo size. Logos render at <=64px in both
/// rails so even 256px PNGs comfortably fit; the cap is set well
/// below "abuse" without preventing reasonable uploads.
const MAX_LOGO_BYTES: usize = 512 * 1024;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/orgs/:org_id/logo",
        axum::routing::get(get_logo)
            .post(upload_logo)
            .delete(delete_logo),
    )
}

#[derive(Debug, Serialize)]
struct UploadResponse {
    /// Path the org's `logo_url` is now set to. Includes a sha256-
    /// prefixed query so a re-upload busts client caches without
    /// changing the path.
    logo_url: String,
    content_type: String,
    sha256: String,
    bytes: usize,
}

async fn upload_logo(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path(org_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiError> {
    require_org_admin(&state.db, ctx.account_id, org_id).await?;

    // Read the first file part. We accept either `file` or unnamed
    // (some <input type="file"> implementations omit the name).
    let mut bytes: Option<Vec<u8>> = None;
    let mut content_type: Option<String> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::InvalidArgument(format!("multipart parse: {e}")))?
    {
        let ct = field.content_type().map(str::to_string);
        let data = field
            .bytes()
            .await
            .map_err(|e| ApiError::InvalidArgument(format!("multipart read: {e}")))?;
        if data.is_empty() {
            continue;
        }
        bytes = Some(data.to_vec());
        content_type = ct;
        break;
    }
    let bytes = bytes
        .ok_or_else(|| ApiError::InvalidArgument("no file part in upload".into()))?;
    if bytes.len() > MAX_LOGO_BYTES {
        return Err(ApiError::InvalidArgument(format!(
            "logo exceeds {MAX_LOGO_BYTES}-byte cap (got {} bytes)",
            bytes.len()
        )));
    }

    // Magic-byte sniff. The `Content-Type` header on a multipart part
    // is client-supplied, so we don't trust it for storage; we infer
    // from the leading bytes and store *that*.
    let inferred = sniff_image(&bytes).ok_or_else(|| {
        ApiError::InvalidArgument(
            "logo must be a PNG, JPEG, GIF, or WEBP image".into(),
        )
    })?;
    let _ = content_type; // header is informational; sniffed value wins.

    let digest = sha256_hex(&bytes);
    let logo_url = format!("/v1/orgs/{org_id}/logo?v={}", &digest[..16]);

    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO org_logos (org_id, content_type, bytes, sha256, updated_at)
        VALUES ($1, $2, $3, $4, now())
        ON CONFLICT (org_id) DO UPDATE SET
            content_type = EXCLUDED.content_type,
            bytes        = EXCLUDED.bytes,
            sha256       = EXCLUDED.sha256,
            updated_at   = now()
        "#,
    )
    .bind(org_id)
    .bind(inferred)
    .bind(&bytes)
    .bind(&digest)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE organizations SET logo_url = $1 WHERE id = $2")
        .bind(&logo_url)
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(UploadResponse {
        logo_url,
        content_type: inferred.to_string(),
        sha256: digest,
        bytes: bytes.len(),
    }))
}

async fn get_logo(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path(org_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // Any org member can read the logo. We don't gate by role so the
    // org rail (cookie-authed `<img>`) renders for every member.
    require_org_membership(&state.db, ctx.account_id, org_id).await?;

    let row: Option<(String, Vec<u8>, String)> = sqlx::query_as(
        "SELECT content_type, bytes, sha256 FROM org_logos WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((content_type, bytes, sha256)) = row else {
        return Err(ApiError::NotFound);
    };

    let etag = format!("\"{sha256}\"");
    if let Some(if_none) = headers.get(header::IF_NONE_MATCH) {
        if if_none.as_bytes() == etag.as_bytes() {
            return Ok(Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &etag)
                .body(Body::empty())
                .map_err(|e| ApiError::Internal(Box::new(e)))?);
        }
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&content_type)
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
        )
        .header(header::ETAG, &etag)
        .header(header::CACHE_CONTROL, "private, max-age=300")
        .body(Body::from(bytes))
        .map_err(|e| ApiError::Internal(Box::new(e)))
}

async fn delete_logo(
    State(state): State<AppState>,
    Extension(ctx): Extension<SessionContext>,
    Path(org_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_org_admin(&state.db, ctx.account_id, org_id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM org_logos WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE organizations SET logo_url = NULL WHERE id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- helpers ----------

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

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

/// Sniff a small set of common image formats from leading magic
/// bytes. Returns the canonical `Content-Type` to store. We accept
/// PNG, JPEG, GIF, and WEBP — covers every "I exported this from
/// Figma" path admins are likely to take.
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_png() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        assert_eq!(sniff_image(&png), Some("image/png"));
    }

    #[test]
    fn sniff_jpeg() {
        let jpg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0];
        assert_eq!(sniff_image(&jpg), Some("image/jpeg"));
    }

    #[test]
    fn sniff_webp() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(b"WEBPVP8 ");
        assert_eq!(sniff_image(&bytes), Some("image/webp"));
    }

    #[test]
    fn sniff_rejects_text() {
        assert_eq!(sniff_image(b"<svg>"), None);
        assert_eq!(sniff_image(b"#!/bin/sh"), None);
    }
}
