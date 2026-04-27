# Phase 3 platform — orgs, teams, onboarding, keychain (planned)

The platform-foundation slice of Phase 3. Bundles the work pushed
out of Phase 2 once 2b.5 narrowed: the Org-and-Teams build (recast
from "team workspaces" after the 2026-04-27 architecture review),
web onboarding chat (formerly 2b.4 follow-up), and OS keychain
caching for the desktop app (formerly 2b.3 follow-up). Sits between
2b.5 wrap and Phase 3a (rebrand + tasks), because the rebrand sweep
should not interleave with active feature work and these items
gate the team-SaaS use cases.

**Starts when:** Phase 2b.5 closes (cookie/CSRF auth + OAuth PKCE
+ MCP HTTP/SSE + `context.propose` all landed) — closed
2026-04-27.

**Prereqs:** 2b.5 complete. Cookie auth in particular gives us
the foundation for invite-link redemption + approval-queue flows.

Live status in [`../implementation-status.md`](../implementation-status.md);
forward scope continues in
[`phase-3a-rebrand-tasks.md`](phase-3a-rebrand-tasks.md).

---

## Goals

1. **Org foundation (Slice 1).** First user → owner of the org;
   pending-signup approval queue gates every other connection;
   org context docs (brand, mission, top-level goals) writable by
   `owner` / `admin` / `org_editor`; multi-org switcher in
   desktop + web. The "Organization" is the user-facing concept
   above the existing storage tenant; v1 maps 1:1.
2. **Teams (Slice 2).** Logical groupings inside the org. Team
   manager role; team-scoped docs (`team_id` tag inside the org
   tenant, gated by `visibility: team` + team membership); team
   context docs.
3. **Web onboarding chat (Slice 3).** Web client gets parity with
   desktop's `OnboardingView.tsx` — an Anthropic-mediated
   conversation that seeds the new account's vault. Requires a
   server-side chat route since the browser can't hold an
   Anthropic API key.
4. **OS keychain — desktop (Slice 4).** Replace plaintext storage
   of the desktop's Anthropic API key and remote session tokens
   in `~/.orchext/` with the OS keychain (`keyring` crate).
   Required before any Phase 4 distribution build.

## What was originally elsewhere

| Item | Was | Moved here on |
|---|---|---|
| Team workspaces (recast as Org + Teams, see D17c–D17g) | Phase 2c | 2026-04-25 |
| Invite flow (join code or email link) | Phase 2c | 2026-04-25 |
| `org/` seed type + `org:` visibility | Phase 2c | 2026-04-25 |
| Web onboarding chat | 2b.4 follow-up | 2026-04-25 |
| OS keychain (desktop) | 2b.3 follow-up | 2026-04-25 |
| Org-above-tenant + approval queue + team_manager + multi-org | architecture review | 2026-04-27 |

D10 / D11 in [`phase-2-plan.md`](phase-2-plan.md) are superseded
by the Phase 3 versions below — the older framing (team == tenant,
three-role model, no approval queue) is preserved in that doc as
a snapshot of the Phase 2-era plan and should not be relied on for
implementation.

## Architectural decisions

**D10 (revised). Organization is the user-facing concept above
the storage tenant.** v1 maps 1:1 — one `organizations` metadata
row per `kind='org'` tenant. Personal vaults stay as
`kind='personal'` per-user. The schema leaves room for many-orgs-
per-tenant or many-tenants-per-org later if a customer needs it;
no current driver for that decoupling. The term "tenant" stays
internal — UI and docs say "Organization" and "personal vault".

**D11 (revised). Roles split across two dimensions.**

- *Org-level* (`memberships(account_id, org_id, role)`):
  `owner` (billing-equivalent + member-mgmt + org-write),
  `admin` (member-mgmt + org-write),
  `org_editor` (org-write only — no member-mgmt),
  `member` (read + propose).
- *Team-level* (`team_memberships(account_id, team_id, role)`):
  `manager` (writes team context, manages own team's membership),
  `member` (reads team context).

First user of a new org → `owner` automatically. Org admins can
manage any team's membership; team managers can manage only their
own team. No per-document ACLs.

**D17a (unchanged).** Invite-link redemption, not email-first.
First cut uses shareable join codes (UUID v4 in URL fragment,
server-recorded with TTL + role + tenant). Single-use, default
7-day expiry. Email delivery requires SMTP + deliverability
story — defer until D17e ships.

**D17b (unchanged).** Onboarding chat goes through the server,
not direct. Web has no Tauri-equivalent escape hatch for an
Anthropic key. Server adds `POST /v1/onboarding/chat` and
`POST /v1/onboarding/finalize` proxying to Anthropic with a
server-held key. Falls into the same shape Phase 3d agent
observer needs anyway, so this earns its keep beyond onboarding.

**D17c. Teams are logical groupings, not separate vaults.** Team-
scoped docs live in the org tenant with a `team_id` tag and
`visibility: team`. No separate audit chain, no separate session
key, no separate storage. The privacy guarantee is at the
visibility-filter + DB-access threat-model layer, not the cipher.
Cryptographic per-team separation rides Phase 3e.3 if a customer
asks (the team session-key keychain slot is already on Phase
3e.3's backlog).

**D17d. Approval queue gates every connection at launch.** Both
self-hosted and SaaS go through the same flow: user signs up →
`pending_signups` row → admin reviews and approves (with optional
team assignment) → membership row created at chosen role. No path
to membership exists outside this queue (or D17a invite codes,
which short-circuit it for known invitees) until D17e ships.

**D17e. Domain-based auto-join — deferred to email infra slice.**
The `organizations.allowed_domains` column lands now (settings UI
hides it until SMTP is wired) but the auto-join code path doesn't
run until email verification is in place. Verification email is
the gate — without it, a `mallory@acme.com` signup with no actual
acme.com access would land inside Acme's org. Tracked as a Phase
3 follow-up slice ("Email infra + domain auto-join"), not part
of platform v1 scope. SMTP provider (SES / Postmark / Resend) to
be picked when the slice begins.

**D17f. Multi-org per account.** A user can belong to N orgs
(Slack/Discord-style). Existing `memberships` table is already
many-to-many. UI gets an org switcher in the top bar, mirroring
the current tenant picker but relabeled. Personal vault is always
present alongside.

**D17g. `org_editor` role for narrow org-context-write grants.**
Lets an org admin promote a member to write `org/*` docs without
also granting member-management. Sits between `member` and
`admin` on the org-level role enum. If a third granular capability
appears, revisit and consider a permissions-flag matrix.

## Deliverables

### Slice 1 — Org foundation — **shipped 2026-04-27**
*(Notion: [Org foundation — Done](https://www.notion.so/34b47fdae49a80a09100d7e9ec10afe8) · [Seed `org/` type + visibility — Done](https://www.notion.so/34b47fdae49a80f3aa60c780298ebe07))*

**Status:** Server, web, and desktop all closed 2026-04-27. The
desktop port followed the web by ~2 days (one slice's worth of
commits to bring it to parity); see commit range `d9031ad..21d30e0`.
A separate context-aware token-issuing fix landed alongside on both
clients — was broken in org workspaces because the form offered no
`org` scope checkbox.

**Cuts realized:** invite-code paste modal on desktop deferred to
Phase 4 installer slice (per-OS deep-link work). Domain auto-join +
email verification deferred to a later "email infra" slice (D17e).
Awaiting-approval state on desktop is intentionally transient (no
synthetic pending workspace registry entry); closing the app while
pending requires re-entering the password to check status. OS
keychain stays on Slice 4.

- **Server**
  - New `organizations` table: id, name, logo_url,
    `allowed_domains` JSONB, settings JSONB, created_at.
  - New `pending_signups` table: account_id, org_id,
    requested_at, requested_role, note, status, decided_by,
    decided_at.
  - First-signup bootstrap: signup with no existing org →
    create org + creator becomes `owner`. Subsequent signups →
    `pending_signups` row + an "awaiting approval" account state
    (no org membership = limited surface).
  - `POST /v1/orgs` — create new org (out-of-band; primarily
    SaaS multi-org or self-hosted second-org).
  - `GET /v1/orgs/:org_id`, `PATCH /v1/orgs/:org_id` — read /
    update org metadata (admin/owner for write).
  - `GET /v1/orgs/:org_id/pending`,
    `POST /v1/orgs/:org_id/pending/:account_id/approve`,
    `POST /v1/orgs/:org_id/pending/:account_id/reject` — approval
    queue (admin/owner). Approve takes optional `role` and
    `team_ids[]`.
  - `GET /v1/orgs/:org_id/members`,
    `PATCH /v1/orgs/:org_id/members/:account_id`,
    `DELETE /v1/orgs/:org_id/members/:account_id` — list /
    role-change / remove (admin/owner). `org_editor` added to
    role enum.
  - Role middleware: enforce `org_editor`-or-higher on `org/*`
    doc paths; admin-or-higher on member-mgmt.
  - `org/` seed type + `org:` visibility land here.
  - `POST /v1/orgs/:org_id/invites`, `GET …`, `DELETE …`,
    `POST /v1/invites/:code/accept` — join codes per D17a (lifted
    from existing plan; routes shift from `/v1/t/:tid/invites/*`
    to `/v1/orgs/:org_id/invites/*`).
  - Migration: rename external `tid` URL param to `org_id` once
    the 1:1 mapping lands; internal `tenant_id` column
    untouched.
- **Desktop + web**
  - Org switcher in top bar (replaces tenant picker copy).
  - "Awaiting approval" gate screen for pending users — login
    succeeds but only that screen renders until approved.
  - Members pane (admin/owner): list members, change role,
    remove, approve / reject pending signups, issue invite
    codes.
  - Org settings pane (admin/owner): name, logo,
    `allowed_domains` (greyed-out + "available when email infra
    ships" tooltip).
  - Org context editor surface — write `org/*` docs gated by
    `org_editor`-or-higher.
  - Invite-redemption: `/invite/:code` route (web) / paste-code
    modal (desktop).
- **Crates touched:** `orchext-server` (migrations, routes, role
  middleware), `orchext-vault` (org seed type), `orchext-auth`
  (`org_editor` in role enum + scope mapping), `apps/desktop`,
  `apps/web`.

### Slice 2 — Teams
*([Notion](https://www.notion.so/34b47fdae49a8033bec2e5f0a2eeaf33))*

- **Server**
  - New `teams` table: id, org_id, name, slug, created_at.
  - New `team_memberships` table: team_id, account_id, role
    ∈ {`manager`, `member`}, created_at.
  - New `team_id` column on `documents` (nullable; null = org-
    scoped, non-null = team-scoped).
  - `POST /v1/orgs/:org_id/teams` — create (admin/owner).
  - `GET /v1/orgs/:org_id/teams` — list (members see teams
    they're in + a public team list).
  - `PATCH /v1/orgs/:org_id/teams/:team_id` — rename
    (admin/owner or team manager).
  - `DELETE /v1/orgs/:org_id/teams/:team_id` — delete
    (admin/owner).
  - `POST /v1/orgs/:org_id/teams/:team_id/members`,
    `DELETE …/:account_id` — add / remove (admin/owner or team
    manager for own team).
  - Visibility filter: `visibility: team` doc + `team_id = X`
    means readable only by `team_memberships` for team X
    (admin/owner of the org also pass — see D11).
- **Desktop + web**
  - Teams list pane.
  - Per-team page: members, team-context docs, manager
    controls.
  - Team picker on document creation (default = org-scoped or
    private).
- **Crates touched:** `orchext-server`, `orchext-vault` (any
  team seed types if added), `orchext-auth` (team-role-derived
  scopes), `apps/desktop`, `apps/web`.

### Slice 3 — Web onboarding chat
*([Notion](https://www.notion.so/34d47fdae49a81d6a012e90cbbcb0d0b))*

- **Server**
  - `POST /v1/onboarding/chat` — proxy a turn to Anthropic;
    server holds `ANTHROPIC_API_KEY` env var.
  - `POST /v1/onboarding/finalize` — single-shot Claude call
    that turns the chat into seed `OnboardingSeedDoc[]`.
- **Web**
  - `OnboardingView.tsx` mirroring desktop's flow.
  - Wire into the `LoginView → OrgPicker → Onboarding (if
    empty) → Documents` post-login state machine.

### Slice 4 — OS keychain
*([Notion](https://www.notion.so/34d47fdae49a819c8ce9dd6511989596))*

- **Desktop** (`orchext-desktop` crate)
  - Replace `~/.orchext/anthropic_key` plaintext with `keyring`
    crate writes (per-user, per-host).
  - Replace remote-workspace session token storage in
    `workspaces.json` with keyring-backed lookup keyed by
    workspace id; the JSON file keeps id + name + URL but not
    the secret.
  - Migration: on first run, if a plaintext key exists, move it
    into the keychain and delete the plaintext copy. Log the
    migration to stderr.
- **Crates touched:** `orchext-desktop`. No server changes.

## Cuts — explicit

- **No SCIM / SAML / SSO.** Email + join code only. Federated
  IdP (Google / GitHub / Okta / WorkOS) re-evaluated when first
  enterprise customer asks.
- **No billing.** Org count and seat count uncapped; pricing is
  a SaaS-launch decision.
- **No per-document ACLs.** `visibility` + roles + `team_id`
  cover it.
- **No invite expiry editing.** Set at issuance, can revoke;
  can't extend in place.
- **No org-level audit log split.** Audit chain stays per-tenant
  and contains all org events for that tenant.
- **No SMTP / email delivery at launch.** Domain auto-join,
  email-invite, and email verification all ride a later slice
  (D17e). Approval queue + join codes cover the gap.
- **No cryptographic per-team separation.** Logical visibility
  filter only (D17c). Phase 3e.3 picks this up if a customer
  asks.
- **No granular permissions matrix.** `org_editor` is the one
  granular role we ship; further capability split waits for a
  third use case.

## Open questions

- **Email infra provider.** SMTP provider (SES / Postmark /
  Resend) to pick when the email infra slice begins. Affects
  D17e auto-join, eventual email-based invites, and any
  notifications follow-up.
- **Member display.** Show email or display name in the members
  pane? Probably both, with display name primary.
- **Web onboarding rate limit.** Anthropic costs are real once
  the server is the proxy. Soft per-account daily cap (TBD).
- **Pending-signup notification.** Self-hosted admins need to
  know a request landed. In-app badge first; email notification
  rides D17e.
- **Server-level vs. org-level admin distinction.** v1 collapses
  these (one org per server for self-hosted at launch). Revisit
  if multi-org-per-server emerges as a need — likely a separate
  "instance owner" concept distinct from "org owner".
