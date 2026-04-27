-- Phase 3 platform — Slice 2 follow-on: server-stored org logos.
--
-- The `organizations.logo_url` column from Slice 1 expected an external
-- HTTPS URL. In practice external references render unreliably (CORS,
-- expiring CDN URLs, mixed content), so this migration adds a small
-- bytea-backed store and the `logo_url` column starts pointing at a
-- self-served route (`/v1/orgs/:id/logo`) once an admin uploads a file.
--
-- Storage choice: Postgres bytea, not S3 / disk. Logos are tiny
-- (capped at 512KB by the upload route), there's at most one per org,
-- and putting them in the DB means self-hosters get backups for free
-- with their existing pg_dump. Move to object storage if logos ever
-- expand beyond this single use case.

CREATE TABLE org_logos (
    org_id        UUID        PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    content_type  TEXT        NOT NULL,
    bytes         BYTEA       NOT NULL,
    sha256        TEXT        NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
