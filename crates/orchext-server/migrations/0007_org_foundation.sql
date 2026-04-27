-- Phase 3 platform — Slice 1: Org foundation.
--
-- Adds the Organization metadata layer above the storage tenant
-- (1:1 in v1; FK + UNIQUE leaves room to decouple later) and the
-- pending-signup approval queue that gates every connection at
-- launch (D17d). Also extends two CHECK enums:
--   * tenants.kind   — adds 'org' alongside the existing 'personal'
--                      and (legacy, unused after the 2026-04-27
--                      architecture review) 'team'.
--   * memberships.role — adds 'org_editor' (D17g) for narrow
--                        org-context-write grants without granting
--                        member-management.
--
-- See docs/phases/phase-3-platform.md for full design (D10/D11
-- revised, D17a–g).

-- Extend tenants.kind. 'team' retained for backwards compatibility
-- of the existing schema even though the new model treats teams as
-- logical groupings inside the org tenant (D17c) — no DDL churn for
-- a value we simply don't write.
ALTER TABLE tenants DROP CONSTRAINT tenants_kind_check;
ALTER TABLE tenants ADD CONSTRAINT tenants_kind_check
    CHECK (kind IN ('personal', 'team', 'org'));

-- Extend memberships.role with 'org_editor'.
ALTER TABLE memberships DROP CONSTRAINT memberships_role_check;
ALTER TABLE memberships ADD CONSTRAINT memberships_role_check
    CHECK (role IN ('owner', 'admin', 'org_editor', 'member'));

-- Organization metadata. One row per org; UNIQUE on tenant_id pins
-- the v1 1:1 mapping. allowed_domains is reserved for D17e (deferred
-- until email infra ships) — the column lands now so the settings
-- API/UI can read/write it without another migration.
CREATE TABLE organizations (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID        NOT NULL UNIQUE REFERENCES tenants(id) ON DELETE CASCADE,
    name            TEXT        NOT NULL,
    logo_url        TEXT,
    allowed_domains JSONB       NOT NULL DEFAULT '[]'::jsonb,
    settings        JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Approval queue. A signup that doesn't bootstrap a new org lands
-- here; the org's admin/owner approves with an optional role +
-- team-id list, which materializes a memberships row. UNIQUE on
-- (account_id, org_id) prevents duplicate requests for the same
-- pair — re-applying after a reject requires the admin to delete
-- the old row first (or we add a "reapply" endpoint later).
CREATE TABLE pending_signups (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id      UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    org_id          UUID        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    requested_role  TEXT        NOT NULL DEFAULT 'member'
                                CHECK (requested_role IN ('owner', 'admin', 'org_editor', 'member')),
    note            TEXT,
    status          TEXT        NOT NULL DEFAULT 'pending'
                                CHECK (status IN ('pending', 'approved', 'rejected')),
    decided_by      UUID        REFERENCES accounts(id) ON DELETE SET NULL,
    decided_at      TIMESTAMPTZ,
    requested_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, org_id)
);

-- Admin queue query: "all pending signups for this org, oldest first".
CREATE INDEX pending_signups_org_status_idx
    ON pending_signups (org_id, status, requested_at);
