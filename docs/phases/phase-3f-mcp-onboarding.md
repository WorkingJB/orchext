# Phase 3f — One-click MCP onboarding for Claude + ChatGPT + Copilot (planned)

The smallest piece of work that turns Orchext's existing MCP HTTP/SSE
surface into something a non-technical user can actually wire into the
two big consumer LLM products without reading a spec. **Next session
priority** as of 2026-04-27.

**Prereqs:** OAuth 2.1 + PKCE *desktop-shaped* code flow (phase 2b.5,
shipped — `POST /v1/oauth/authorize` JSON, session-authed); MCP HTTP
transport at `POST /v1/mcp` (phase 2b.5, shipped); orgs + teams +
tokens UI (phase 3 platform Slices 1+2, shipped 2026-04-27). MCP SSE
was *deferred* in 2b.5 and lands here as 3f.0; the existing
`POST /v1/oauth/authorize` is preserved for desktop and a new
redirect-based `GET /v1/oauth/authorize` is added for provider
browser flows.

**Independent of:** Phase 3 platform Slices 3 (web onboarding chat) and
4 (OS keychain). Both can land in any order relative to this work.

Live status in [`../implementation-status.md`](../implementation-status.md).

---

## The user-visible problem

All three providers' "add custom MCP server" flows ask for some
combination of the same fields:

| | Claude.ai → *Add custom connector* | ChatGPT → *New App* (custom MCP) | Copilot Studio → *Add MCP server* |
|---|---|---|---|
| Name | required | required | required |
| Description | — | optional | optional |
| Server URL | required ("Remote MCP server URL") | required ("https://example.com/sse") | required (server endpoint) |
| OAuth Client ID | optional | inside *Advanced OAuth settings* | required when auth = OAuth |
| OAuth Client Secret | optional | inside *Advanced OAuth settings* | required when auth = OAuth |
| Risk acknowledgement | implicit | explicit checkbox | tenant-admin gated |

Today an Orchext user has to:
1. Find/guess our server URL (none of the existing UI tells them).
2. Decide whether to use OAuth or bearer auth, and read MCP.md to know
   what's accepted.
3. Issue a token by hand, or register an OAuth client by hand.
4. Copy the right values into the right fields without a typo.

The goal of this slice: a single **Connect to Claude** / **Connect to
ChatGPT** / **Connect to Copilot** affordance in Settings that
produces every value those forms need, with copy-to-clipboard
buttons, a deep link to the provider's add-connector page, and a
verification step that confirms the connection actually works.

## Goals

1. **Discoverable.** A "Connectors" tab in Settings (web + desktop)
   lists "Claude.ai", "ChatGPT", "Copilot", and "Custom (manual)".
   No hunting.
2. **Per-provider wizard.** Each tile opens a modal that:
   - Shows the URL to paste, with a copy button.
   - Generates an OAuth client (or token, for the manual path) bound
     to the active org + scope, with copy buttons for each field.
   - Links out to the provider's add-connector URL.
   - Polls for the first successful MCP request from that client and
     flips to "Connected ✓" without a refresh.
3. **One server URL, two shapes.** ChatGPT's UI hard-codes the suffix
   `/sse`; Claude is permissive. Server exposes both
   `https://app.orchext.ai/v1/mcp` (current) **and**
   `https://app.orchext.ai/v1/mcp/sse` as an alias, so the user can
   paste whichever the provider expects without us shipping a second
   transport.
4. **Per-org default scope, per-connector override.** Default scope on
   wizard creation is the same as the user's existing token-issuing
   scope (`read` / `read_propose`, the org's `visibility` set). User
   can narrow before generating; can't widen beyond their role.
5. **Audited like any other token.** Every wizard-generated client
   shows up in the existing Tokens pane. Revocation is one click.
   Connection origin is recorded so the Tokens list can label
   "Claude.ai connector — laptop" automatically.

## Architectural decisions

**D43. OAuth Dynamic Client Registration ([RFC 7591]) is the
provider-agnostic path.** Both Claude and ChatGPT can be wired
without a pre-registered client by typing the server URL alone — the
provider hits `/.well-known/oauth-authorization-server`, dynamically
registers, and walks the PKCE flow. We already speak OAuth 2.1; this
adds the metadata endpoints + a registration endpoint and lets the
"easy path" be **paste server URL, click Add**. The "advanced
OAuth client id + secret" fields in either provider become a fallback
for users who want a long-lived pre-registered client.

[RFC 7591]: https://datatracker.ietf.org/doc/html/rfc7591

**D44. There must be an `oauth_clients` table.** Today there isn't
one — `POST /v1/oauth/authorize` is session-authed and issues
`mcp_tokens` directly. RFC 7591 returns `client_id` + `client_secret`
that providers replay against `/authorize` and `/token`, so the table
is mandatory, not "add an origin column." Schema lands in 3f.1:
`(client_id PK, client_secret_prefix, client_secret_hash, client_name,
redirect_uris[], origin, tenant_id, account_id, default_scope[],
default_mode, created_at, revoked_at, last_used_at)`. `mcp_tokens`
gains a nullable `oauth_client_id` FK so connector-issued tokens
back-link cleanly. `oauth_authorization_codes` gains a nullable
`client_id` so codes minted by the redirect flow remember their
client.

**D45. Ship MCP SSE in 3f.0; the `/sse` "alias" then becomes a
second route on the same handler.** 2b.5 shipped only `POST /v1/mcp`
— SSE was explicitly deferred (`mcp.rs:6-10`). Provider connectors
need a streamable-HTTP server channel, so 3f.0 adds
`GET /v1/mcp` (`text/event-stream`) and `GET /v1/mcp/sse` as a second
mount of the same handler so ChatGPT's hard-coded `/sse` suffix works
without a second transport. Documented in MCP.md.

**D47. Add a redirect-based `GET /v1/oauth/authorize`.** The existing
`POST /v1/oauth/authorize` is the *desktop* flow: a logged-in user
calls JSON from inside an Orchext session and receives a code in the
response body. Providers cannot speak that. They redirect a *browser*
to `GET /v1/oauth/authorize?response_type=code&client_id=…&redirect_uri=…&code_challenge=…&state=…`,
expect a consent UI when the user is logged in (or a 302 to login
when they aren't, with `?next=` bounce-back), and then expect a 302
back to `redirect_uri` carrying `code` + `state`. We add this
endpoint, validate `client_id` + `redirect_uri` against `oauth_clients`,
require PKCE (`S256`-only), and reuse the existing
`oauth_authorization_codes` table. `POST /v1/oauth/token` learns to
authenticate the registered client via Basic auth (RFC 6749 §2.3.1)
when the code carries a `client_id`; the desktop POST-authorize codes
keep working unchanged because their `client_id` column is null.

**D46. Self-hosters get the same wizard.** The wizard reads the
configured `BASE_URL` env var (already set on the Fly app) and
substitutes it into the displayed URL. Self-hosters who run on
`orchext.example.com` see their own host in the copy field. No
SaaS-specific code paths.

## Sub-milestones

### Phase 3f.0 — MCP streamable-HTTP server channel (skeleton)

**Why first:** providers wire `…/sse`-suffixed URLs into their forms
and open a GET channel as part of the handshake. We need the channel
to *exist* and authenticate; we do *not* yet need it to push
`notifications/*` because no provider consumes those today. Push
lands as 3f.0b when a real driver shows up.

**Deliverables (narrowed):**

- `GET /v1/mcp` returns `text/event-stream` after Bearer-auth
  resolves the same `mcp_tokens` row the POST surface uses. Sends a
  `:keepalive` comment line every ~25s. Drops on token revocation
  (best-effort: connection lives until next keepalive tick fails to
  re-resolve, or until the client disconnects).
- `GET /v1/mcp/sse` is a second mount of the same handler so
  ChatGPT's hard-coded `/sse` suffix works without a second
  transport. Documented in MCP.md.
- 401 on missing/invalid bearer matches the POST surface (uniform
  `WWW-Authenticate: Bearer` header to be added in 3f.1 alongside
  the resource-metadata pointer).

**Verification:**

- `curl -N -H 'Authorization: Bearer …' https://test-app.orchext.ai/v1/mcp/sse`
  receives a `:keepalive` comment within ~30s and the connection
  stays open.
- `curl /v1/mcp/sse` with no Authorization → `401`.
- Both routes hit the same handler — integration test asserts byte-
  identical first-frame from `/v1/mcp` and `/v1/mcp/sse`.

### Phase 3f.0b — `notifications/*` push fan-out (deferred)

Lands when a provider's MCP client actually consumes
`notifications/resources/updated` to refresh its tool surface. Likely
shape: Postgres `LISTEN/NOTIFY` keyed per tenant, document-write
paths emit `pg_notify`, SSE handler bridges them onto its stream.
Skipped for 3f.0 because none of Claude / ChatGPT / Copilot consume
the events today and the plumbing isn't load-bearing for the wizard.

### Phase 3f.1 — OAuth client registry + discovery + redirect authorize

**Deliverables:**

- Migration: create `oauth_clients` table with the schema in D44.
  Add `oauth_client_id UUID NULL REFERENCES oauth_clients(client_id)`
  to `mcp_tokens`. Add `client_id UUID NULL REFERENCES
  oauth_clients(client_id)` to `oauth_authorization_codes`.
- `GET /.well-known/oauth-authorization-server` — issuer metadata
  per [RFC 8414]. Lists authorization endpoint, token endpoint,
  registration endpoint, supported PKCE methods (`S256` only),
  supported scopes (`read`, `read_propose`, plus per-org visibility
  labels), `code_challenge_methods_supported = ["S256"]`.
- `GET /.well-known/oauth-protected-resource` — resource metadata
  per [RFC 9728] published at the resource URL so clients can
  discover the authorization server from any `/v1/mcp` 401 response
  (`WWW-Authenticate: Bearer resource_metadata="…"`).
- `POST /v1/oauth/register` — dynamic client registration per
  [RFC 7591]. Accepts `redirect_uris`, `client_name`. Returns
  `client_id`, `client_secret`, registration metadata. Rate-limited
  per IP. Tags new clients `origin = 'dynamic_registration'`.
- `GET /v1/oauth/authorize` — redirect-based authorization endpoint
  per D47. Renders a minimal consent screen (server-rendered HTML;
  the SPA doesn't own this URL because providers redirect *browsers*
  here directly). `Allow {client_name} to access {org} with
  {scope}?` + Approve / Deny. Approve → mint code →
  `302 redirect_uri?code=…&state=…`. Deny →
  `302 redirect_uri?error=access_denied&state=…`. Unauthenticated
  user → `302 /login?next=/v1/oauth/authorize?…` (re-encoded).
- `POST /v1/oauth/token` — extend to authenticate the registered
  client via Basic auth (`Authorization: Basic …`,
  `client_id:client_secret`) when the code's `client_id` is set.
  Existing desktop codes (no `client_id`) keep working with no auth.
- Connector-bound client provisioning: an internal helper that the
  wizard calls (in 3f.2) to create an `oauth_clients` row tagged
  `origin = 'claude_connector'` / `chatgpt_connector' /
  'copilot_connector'` with the right `redirect_uris[]` baked in.
- Tokens pane / API: surface `origin` (via the `oauth_clients`
  back-link) so connector-issued tokens render as "Claude.ai
  connector" / "Copilot connector". Manual tokens render as before.

[RFC 8414]: https://datatracker.ietf.org/doc/html/rfc8414
[RFC 9728]: https://datatracker.ietf.org/doc/html/rfc9728

**Verification:**

- `curl https://test-app.orchext.ai/.well-known/oauth-authorization-server`
  returns valid JSON metadata; `curl /.well-known/oauth-protected-resource`
  matches.
- Register a client via `curl POST /v1/oauth/register`. Drive the
  full redirect flow with [`@modelcontextprotocol/inspector`] (or
  `curl --location` against `/v1/oauth/authorize` after seeding a
  session cookie): get redirected with `code` + `state`, exchange
  with Basic auth, hit `/v1/mcp` with the bearer, see a successful
  `initialize` response.
- Existing desktop POST-authorize → token flow still passes the
  current oauth integration tests with no changes.
- Tokens pane lists the new connector-issued token with the
  "Claude.ai connector" label.

### Phase 3f.2 — Web + Desktop: Connectors tab + per-provider wizards

**Deliverables:**

- `apps/web/src/ConnectorsView.tsx` (new) and the desktop mirror.
  Four cards: Claude.ai, ChatGPT, Copilot, Custom.
- Per-provider wizard component:
  - Step 1: pick scope (`read` / `read_propose`) + visibility set
    (defaults to org's read-set).
  - Step 2: server creates a pre-registered OAuth client tagged
    with the right `origin`. Wizard shows the server URL + the
    Client ID + (revealed-once) Client Secret with copy buttons,
    and a "Take me to claude.ai" / "Take me to chatgpt.com" /
    "Take me to Copilot Studio" deep link.
  - Step 3: poll `GET /v1/oauth/clients/:id/last_used` every 3s.
    On first non-null `last_used`, flip to "Connected" + close.
- "Disconnect" action revokes the client + every token issued
  against it. Mirrors current token-revoke UX.
- Add a "Connectors" sub-tab in Settings between "Tokens" and
  "Audit" for both web + desktop.
- Update the existing Tokens pane copy: "OAuth clients you
  registered through a connector show up here too."

**Verification:**

- Click *Connect to Claude* → modal → click *Take me to claude.ai*
  in incognito → paste pre-filled URL → walk OAuth → wizard flips
  to Connected within ~5s after first MCP call.
- Same for ChatGPT, including paste of `…/v1/mcp/sse`.
- Same for Copilot Studio: pre-registered Client ID + Secret paste
  cleanly into its OAuth form; first `tools/list` flips wizard to
  Connected.
- Disconnect button revokes; subsequent MCP calls 401.
- E2E test with the [`@modelcontextprotocol/inspector`] CLI as a
  stand-in (no Claude account required for CI).

[`@modelcontextprotocol/inspector`]: https://github.com/modelcontextprotocol/inspector

## Cuts — explicit

- **No deep PKCE-by-default for the manual path.** Manual still
  works (bearer token + scope). The wizard is the easy path; manual
  stays as the escape hatch.
- **Consent screen is functional, not designed.** Server-rendered
  HTML, Orchext branding, two buttons, no SPA bundle on this URL.
  Providers redirect a browser here once per connector setup; the
  user-experience anchor is the wizard, not the consent screen.
- **No per-tool consent screens.** Scope is org × visibility set,
  same model as today's tokens. Per-tool granular consent is a
  later slice if a customer asks.
- **No "test the connection" button before the provider has hit us
  once.** The wizard polls `last_used`; we don't synthesize a
  request to validate. Adds round-trip but matches what the user
  actually wants confirmed (a real call).
- **No connector for non-MCP integrations** (Cursor, Continue, raw
  curl). Those continue to use the existing Tokens UI. Reassess
  once the provider list stabilizes.
- **No refresh tokens in 3f.** Access tokens keep the existing
  90-day default. Refresh-token rotation is a real OAuth 2.1 ask;
  defer until a provider's MCP client actually requires it.

## Open questions

- **Copilot deep-link target.** Copilot Studio's *Add MCP server*
  URL is tenant-scoped (`copilotstudio.microsoft.com/.../tenant/...`);
  the wizard probably can't deep-link to a single canonical page the
  way Claude/ChatGPT can. Lean: link to the public docs landing page
  and let admins navigate the last hop, with a tooltip explaining
  why. Confirm during 3f.2 build.
- **Auto-register Orchext as an MCP provider with Claude.ai's
  directory?** Anthropic has a private list. Defer until SaaS
  launch; first 100 users typing the URL by hand is fine.
- **Display name fallback when `BASE_URL` is unset on self-host.**
  Current desktop has a `server_url` per workspace; the wizard
  should prefer that. For SaaS, `BASE_URL` is mandatory in
  `deploy/fly/orchext-prod.toml`.
- **OAuth client lifetime.** Default to no expiry on the client
  itself; tokens issued against it have the existing 90-day default.
  Revocation is the kill-switch, not expiry.
- **Connection nickname capture.** Wizard's *Name* field defaults
  to "Claude.ai" / "ChatGPT" — should we let the user override at
  creation, or only edit later via the Tokens pane? Lean on the
  latter; one fewer step in the wizard.
