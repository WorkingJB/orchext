-- Phase 3 platform — Slice 2: Teams.
--
-- Teams are logical groupings inside an org tenant (D17c) — not
-- separate vaults, no separate audit chain or session key. This
-- migration adds:
--   * `teams` — id, org_id, name, slug, created_at.
--   * `team_memberships` — (team_id, account_id) PK + role
--     ∈ {manager, member}.
--   * `documents.team_id` — nullable FK; the visibility filter reads
--     `visibility = 'team' AND team_id = X` and checks
--     `team_memberships`. CHECK constraint pins the strict coupling
--     `team_id IS NOT NULL ⟺ visibility = 'team'` so docs can never
--     drift into a half-team-half-org state.
--
-- See docs/phases/phase-3-platform.md (Slice 2 + D11 + D17c).

CREATE TABLE teams (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id      UUID        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        TEXT        NOT NULL,
    slug        TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, slug)
);

CREATE INDEX teams_org_idx ON teams (org_id, created_at);

CREATE TABLE team_memberships (
    team_id     UUID        NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    account_id  UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role        TEXT        NOT NULL DEFAULT 'member'
                            CHECK (role IN ('manager', 'member')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (team_id, account_id)
);

-- Reverse lookup: "all teams this account belongs to in any org" —
-- used by the visibility filter on document reads.
CREATE INDEX team_memberships_account_idx
    ON team_memberships (account_id, team_id);

ALTER TABLE documents
    ADD COLUMN team_id UUID REFERENCES teams(id) ON DELETE CASCADE;

-- Strict coupling: a doc with visibility='team' must name a team, and
-- a doc that names a team must be visibility='team'. Ruling out the
-- partial states keeps the visibility filter one-dimensional —
-- callers don't have to reason about a doc that's tagged "org" but
-- carries a stray team_id.
ALTER TABLE documents
    ADD CONSTRAINT documents_team_visibility_check
    CHECK ((visibility = 'team') = (team_id IS NOT NULL));

-- Filter shape on doc list / read: "team docs in this tenant for this
-- team". Partial index keeps it small — most rows have NULL team_id.
CREATE INDEX documents_team_idx
    ON documents (tenant_id, team_id)
    WHERE team_id IS NOT NULL;
