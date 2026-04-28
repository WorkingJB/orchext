# Phase 3f — One-click MCP onboarding for Claude + ChatGPT (planned)

The smallest piece of work that turns Orchext's existing MCP HTTP/SSE
surface into something a non-technical user can actually wire into the
two big consumer LLM products without reading a spec. **Next session
priority** as of 2026-04-27.

**Prereqs:** OAuth 2.1 + PKCE for agent tokens (phase 2b.5, already
shipped); MCP HTTP/SSE transport at `POST /v1/mcp` + `GET /v1/mcp/events`
(phase 2b.5, already shipped); orgs + teams + tokens UI (phase 3
platform Slices 1+2, shipped 2026-04-27).

**Independent of:** Phase 3 platform Slices 3 (web onboarding chat) and
4 (OS keychain). Both can land in any order relative to this work.

Live status in [`../implementation-status.md`](../implementation-status.md).

---

## The user-visible problem

Both providers' "add custom MCP server" flows ask for:

| | Claude.ai → *Add custom connector* | ChatGPT → *New App* (custom MCP) |
|---|---|---|
| Name | required | required |
| Description | — | optional |
| Server URL | required ("Remote MCP server URL") | required ("https://example.com/sse") |
| OAuth Client ID | optional | inside *Advanced OAuth settings* |
| OAuth Client Secret | optional | inside *Advanced OAuth settings* |
| Risk acknowledgement | implicit | explicit checkbox |

Today an Orchext user has to:
1. Find/guess our server URL (none of the existing UI tells them).
2. Decide whether to use OAuth or bearer auth, and read MCP.md to know
   what's accepted.
3. Issue a token by hand, or register an OAuth client by hand.
4. Copy the right values into the right fields without a typo.

The goal of this slice: a single **Connect to Claude** / **Connect to
ChatGPT** affordance in Settings that produces every value those
forms need, with copy-to-clipboard buttons, a deep link to the
provider's add-connector page, and a verification step that confirms
the connection actually works.

## Goals

1. **Discoverable.** A "Connectors" tab in Settings (web + desktop)
   lists "Claude.ai", "ChatGPT", and "Other (manual)". No hunting.
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

**D44. The connector wizard issues a **bound** OAuth client.** Each
"Connect to Claude" press creates one `oauth_clients` row tagged
`origin = 'claude_connector'` (or `chatgpt_connector`), scoped to
the active org + tenant, with `redirect_uri` set per provider. The
user never picks a redirect URI — the wizard knows it.

**D45. SSE alias only; no second transport.** Add a route alias at
`/v1/mcp/sse` that resolves to the same handler as `/v1/mcp/events`.
A single Axum `route(...)` line; nothing else changes. Documented in
MCP.md.

**D46. Self-hosters get the same wizard.** The wizard reads the
configured `BASE_URL` env var (already set on the Fly app) and
substitutes it into the displayed URL. Self-hosters who run on
`orchext.example.com` see their own host in the copy field. No
SaaS-specific code paths.

## Sub-milestones

### Phase 3f.1 — Server: discovery endpoints + SSE alias + connector tagging

**Deliverables:**

- Migration: `oauth_clients` gains `origin TEXT` (nullable; values
  `claude_connector` / `chatgpt_connector` / `manual` /
  `dynamic_registration`). Backfill existing rows to `manual`.
- `GET /.well-known/oauth-authorization-server` — issuer metadata
  per [RFC 8414]. Lists authorization endpoint, token endpoint,
  registration endpoint, supported PKCE methods, supported scopes.
- `GET /.well-known/oauth-protected-resource` — resource metadata
  per [RFC 9728]. Required by recent MCP clients to discover the
  authorization server from the resource URL.
- `POST /v1/oauth/register` — dynamic client registration per
  [RFC 7591]. Accepts `redirect_uris`, `client_name`. Returns
  `client_id`, `client_secret`, registration metadata. Rate-limited
  per IP. Tags new clients `origin = 'dynamic_registration'`.
- Route alias `GET /v1/mcp/sse` → existing SSE handler.
- Tokens pane / API: surface `origin` so connector-issued tokens
  render as e.g. "Claude.ai connector".

[RFC 8414]: https://datatracker.ietf.org/doc/html/rfc8414
[RFC 9728]: https://datatracker.ietf.org/doc/html/rfc9728

**Verification:**

- `curl https://test-app.orchext.ai/.well-known/oauth-authorization-server`
  returns valid JSON metadata.
- Register a client via `curl POST /v1/oauth/register`, walk
  authorization code + PKCE, exchange for token, hit `/v1/mcp` with
  `Authorization: Bearer …`, get a successful initialize response.
- Tokens pane lists the new client with origin = "API" or
  "Claude.ai connector" label.

### Phase 3f.2 — Web + Desktop: Connectors tab + per-provider wizards

**Deliverables:**

- `apps/web/src/ConnectorsView.tsx` (new) and the desktop mirror.
  Three cards: Claude.ai, ChatGPT, Custom.
- Per-provider wizard component:
  - Step 1: pick scope (`read` / `read_propose`) + visibility set
    (defaults to org's read-set).
  - Step 2: server creates a pre-registered OAuth client tagged
    with the right `origin`. Wizard shows the server URL + the
    Client ID + (revealed-once) Client Secret with copy buttons,
    and a "Take me to claude.ai" / "Take me to chatgpt.com" deep
    link.
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
- Disconnect button revokes; subsequent MCP calls 401.
- E2E test with the [`@modelcontextprotocol/inspector`] CLI as a
  stand-in (no Claude account required for CI).

[`@modelcontextprotocol/inspector`]: https://github.com/modelcontextprotocol/inspector

## Cuts — explicit

- **No deep PKCE-by-default for the manual path.** Manual still
  works (bearer token + scope). The wizard is the easy path; manual
  stays as the escape hatch.
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

## Open questions

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
