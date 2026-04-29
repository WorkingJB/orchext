-- Phase 3f.1: in-flight authorization requests for the redirect-based
-- GET /v1/oauth/authorize flow.
--
-- The existing POST /authorize (desktop, session-authed JSON) inserts
-- straight into oauth_authorization_codes — the user "consents" by
-- being session-authed and clicking inside the desktop app, so the
-- code is redeemable on insert.
--
-- The redirect flow instead lands the user's browser on a server-
-- rendered consent page with Approve / Deny buttons. We park the
-- request here while we wait for that click. Approve → mint a real
-- oauth_authorization_codes row + 302 to the provider's redirect_uri.
-- Deny → drop the row + 302 with error=access_denied.
--
-- Ephemeral (~10 min TTL) — no relationship to long-lived data; an
-- abandoned consent screen leaves a row that the next probe sweeps.

CREATE TABLE oauth_pending_authorizations (
    request_id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id              UUID        NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    -- The session that initiated the consent screen. Decision-handler
    -- requires the same account_id on the POST so a stolen request_id
    -- can't be redeemed by another logged-in user on the same browser.
    account_id             UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    redirect_uri           TEXT        NOT NULL,
    -- OAuth `state`: opaque to us, echoed back to the provider on
    -- redirect for their CSRF check. NULL is allowed because RFC 6749
    -- only RECOMMENDS state, not requires.
    state                  TEXT,
    scope                  TEXT[]      NOT NULL DEFAULT '{}',
    mode                   TEXT        NOT NULL CHECK (mode IN ('read','read_propose')),
    code_challenge         TEXT        NOT NULL,
    code_challenge_method  TEXT        NOT NULL CHECK (code_challenge_method = 'S256'),
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at             TIMESTAMPTZ NOT NULL
);

CREATE INDEX oauth_pending_authorizations_expires_idx
    ON oauth_pending_authorizations (expires_at);
