-- Phase 3f.1 schema: OAuth client registry for the MCP onboarding
-- wizard and dynamic-client-registration path.
--
-- Today's flow (POST /v1/oauth/authorize, session-authed) issues
-- mcp_tokens directly with no notion of a registered client. Provider
-- onboarding (Claude / ChatGPT / Copilot connectors) and RFC 7591
-- dynamic client registration both require a real client identity
-- that survives the redirect-based authorization code flow added in
-- this phase.
--
-- The wizard creates a row tagged origin = 'claude_connector' /
-- 'chatgpt_connector' / 'copilot_connector' with provider-specific
-- redirect_uris baked in. RFC 7591 DCR creates rows tagged
-- origin = 'dynamic_registration'. The legacy desktop POST-authorize
-- path stays unchanged: its codes have client_id = NULL and never
-- join through oauth_clients.

CREATE TABLE oauth_clients (
    client_id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Argon2id hash + prefix lookup, matching the convention used by
    -- mcp_tokens, sessions, and oauth_authorization_codes.
    client_secret_prefix TEXT        NOT NULL UNIQUE,
    client_secret_hash   TEXT        NOT NULL,
    client_name          TEXT        NOT NULL,
    -- RFC 7591 §2: array of allowed redirect URIs. Validation at
    -- /authorize time exact-matches against this set.
    redirect_uris        TEXT[]      NOT NULL,
    origin               TEXT        NOT NULL CHECK (origin IN (
                            'claude_connector',
                            'chatgpt_connector',
                            'copilot_connector',
                            'dynamic_registration',
                            'manual'
                         )),
    tenant_id            UUID        NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    account_id           UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Wizard-set defaults pre-populated into the consent screen the
    -- provider's user lands on. Subset rules still apply: a code
    -- minted via /authorize cannot widen beyond default_scope.
    default_scope        TEXT[]      NOT NULL DEFAULT '{}',
    default_mode         TEXT        NOT NULL DEFAULT 'read'
                                     CHECK (default_mode IN ('read','read_propose')),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at           TIMESTAMPTZ,
    -- Drives the wizard's "Connected ✓" flip in 3f.2 — touched by
    -- the token-redemption path the same way mcp_tokens.last_used_at
    -- is touched on every /v1/mcp request.
    last_used_at         TIMESTAMPTZ
);

CREATE INDEX oauth_clients_tenant_idx ON oauth_clients (tenant_id);
CREATE INDEX oauth_clients_account_idx ON oauth_clients (account_id);

-- Backlinks. NULL on legacy / desktop / manual rows; populated on
-- the connector + dynamic-registration paths.
--
-- ON DELETE SET NULL on mcp_tokens because token revocation should
-- survive client deletion (audit trail intact). ON DELETE CASCADE on
-- oauth_authorization_codes because an unredeemed code is meaningless
-- once its client is gone.
ALTER TABLE mcp_tokens
    ADD COLUMN oauth_client_id UUID
        REFERENCES oauth_clients(client_id) ON DELETE SET NULL;

ALTER TABLE oauth_authorization_codes
    ADD COLUMN client_id UUID
        REFERENCES oauth_clients(client_id) ON DELETE CASCADE;
