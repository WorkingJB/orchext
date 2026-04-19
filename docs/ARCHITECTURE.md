# Mytex Architecture

> API and AI documentation but for you.

Mytex lets a person define, manage, and transport a living set of context
files about themselves, and connect any AI agent to them — without handing
every agent full access to everything, and without being locked into a
single AI provider's memory system.

This document describes the v1 architecture and the decisions behind it.
It is the starting contract for contributors and should be kept in sync
with the code.

---

## 1. Guiding principles

The design is evaluated against four principles, in order:

1. **User first** — the user owns their data, their keys, and the
   relationship with every agent. No feature ships that weakens that.
2. **Easy for everyone** — a non-technical person can install Mytex,
   onboard in a conversation, and connect an agent in under ten minutes.
3. **Simple over fancy** — boring, auditable, well-understood tech wins
   over novel tech. Plain markdown beats a bespoke database.
4. **Always secure** — the default configuration is the secure
   configuration. Security is not a tier or an upsell.

When these conflict, earlier principles win.

---

## 2. System shape: local-first, sync-optional

The source of truth is a **vault** — a plain directory of markdown files
on the user's machine. Every other component (desktop UI, local API,
MCP server, cloud sync) is a view or transport over that directory.

```
┌──────────────────────────────────────────────────────────────┐
│  Desktop app (Tauri)                                         │
│  ├─ UI                 (TypeScript / React)                  │
│  ├─ Core engine        (Rust — small, audited surface)       │
│  │   ├─ Vault driver                                         │
│  │   ├─ Indexer                                              │
│  │   ├─ Token + permission service                           │
│  │   ├─ Audit log                                            │
│  │   └─ Crypto                                               │
│  ├─ Local API          (HTTPS on 127.0.0.1)                  │
│  └─ MCP server         (stdio + HTTP/SSE)                    │
└──────────────────────┬───────────────────────────────────────┘
                       │  reads / writes
                       ▼
               ~/Mytex/ (vault)
               ├─ .mytex/        config, keys, index, audit
               ├─ identity/
               ├─ goals/
               ├─ relationships/
               ├─ tools/
               └─ …               markdown + YAML frontmatter
                       │
                       │  optional, E2EE
                       ▼
               Cloud tier (paid)
               ├─ Encrypted blob sync
               ├─ MCP/API relay
               └─ Web UI (decrypts in-browser via WASM)
```

**Why local-first**

- The user's disk is the most secure default storage available.
- A vault-as-folder is portable, grep-able, diff-able, and git-able.
- Cloud becomes encrypted transport, not a second system.
- One codebase serves self-hosted and hosted modes.

---

## 3. Key decisions

These are the choices that shape everything else. They should not be
changed without updating this document.

### 3.1 Framework: Tauri with a thin Rust core

The UI is TypeScript/React. A deliberately small Rust core
(filesystem I/O, crypto, token validation, MCP server) is kept
auditable by humans.

**Why:** Tauri's default-deny IPC model catches mistakes that AI-written
code tends to make in Electron (unsafe IPC, `nodeIntegration`, sloppy
preload scripts). The UI — where the bulk of AI-generated code will live
— is still TypeScript. The trusted core is small enough to review line
by line.

**Escape hatch:** the file format, MCP protocol, and API contract are
framework-independent. A port to Electron later would not break any
user's vault.

### 3.2 Storage: pluggable vault driver, plain files first

All vault access goes through a `VaultDriver` interface. v1 ships a
`PlainFileDriver`. A future `EncryptedDriver` (per-file envelope, so
git diffs still work) is a drop-in replacement.

```ts
interface VaultDriver {
  list(path: string): Promise<Entry[]>
  read(id: string): Promise<Document>
  write(id: string, content: Document): Promise<void>
  watch(callback: ChangeHandler): Unsubscribe
}
```

**Why:** users get the portability of plain markdown today, with a
clean path to encryption-at-rest later. Nothing in the UI, API, or MCP
server knows which driver is in use.

### 3.3 Onboarding: in-app agent, external agents read-only

Two trust tiers for writes:

- **In-app onboarding agent** runs inside the desktop app using the
  user's chosen model. Because the user is actively watching the
  conversation, it writes directly to the vault. No MCP, no token, no
  per-write prompt.
- **External agents via MCP** are read-only by default. Writes go
  through `context.propose(id, patch)`, which lands in
  `.mytex/proposals/`. The desktop app surfaces proposals for user
  review; approval merges them.

**Why:** conversational onboarding is the easiest possible UX, and it
doesn't require opening the external write surface to every agent the
user ever connects.

### 3.4 Cloud tier: session-bound decryption for always-on integrations

Cloud storage is end-to-end encrypted at rest. The master key never
leaves user devices. To keep hosted agent integrations always-on
without requiring the desktop to be running, the cloud tier uses
**session-bound decryption**:

- Master key derived from the user's passphrase via Argon2id
  (tuned to ~500 ms on a mid-range laptop). Device-only.
- Files encrypted client-side (libsodium `secretstream` or age) before
  upload. At rest the server sees opaque blobs plus minimal metadata.
- While any user device is online and unlocked, it publishes a
  short-lived **session key** to the cloud, derived from the master
  key and TTL-bound (default: sliding 24h window, refreshed
  automatically while a device is active).
- The relay uses the session key to decrypt on demand and serve agent
  integrations server-side. This is the default integration path for
  cloud-tier users.
- When every device has been offline past the TTL, session keys
  expire and the cloud falls back to opaque blobs. Agents see a
  locked state until a device comes back online.
- Revocation: removing a device or manual "lock cloud" action
  expires the session key immediately.
- Web UI decrypts in-browser via WASM; the server never holds the
  passphrase or the master key.
- Recovery = a one-time recovery code generated at setup, printable.
  Optional Shamir split across trusted contacts. No server-side
  recovery.

**Trust claim.** Mytex cloud can decrypt context only during a session
that a user device authorized. Strict no-cloud-decryption is an opt-in
mode for users who prefer it; their integrations fall back to
relay-to-device (agent sees locked state when desktop is offline).

**UX:** modelled on Bitwarden / 1Password. Passphrase once per
session, cached in the OS keychain, biometric unlock after that.

### 3.5 File format: markdown + YAML frontmatter + wikilinks

See [`FORMAT.md`](./FORMAT.md) for the full spec.

Short version: every context object is a `.md` file with a YAML
frontmatter header declaring `id`, `type`, `visibility`, `tags`,
`links`, and a handful of reserved fields. The body is freeform
markdown. Inter-document references use Obsidian-style `[[wikilinks]]`.

**Why:** Obsidian-compatible on purpose. No lock-in, git-friendly, and
any text editor is a fallback UI.

---

## 4. Components

### 4.1 Vault

A directory on disk with a fixed top-level layout:

```
~/Mytex/
├─ .mytex/
│   ├─ config.json        user preferences, driver selection
│   ├─ tokens.json        hashed agent tokens + scopes
│   ├─ audit.log          append-only, hash-chained
│   ├─ index.sqlite       derived search + graph index
│   └─ proposals/         pending agent-proposed writes
├─ identity/
├─ roles/
├─ goals/
├─ relationships/
├─ memories/
├─ tools/
├─ preferences/
├─ domains/
└─ decisions/
```

The directories under the root map to the seed `type` values. Users may
add their own top-level folders; the indexer treats them as custom
types.

### 4.2 Core engine (Rust)

Responsibilities, kept narrow:

- **Vault driver** — the only code that touches files.
- **Indexer** — watches the vault, rebuilds `index.sqlite`, exposes
  search and graph queries. The SQLite index is derived, never
  authoritative.
- **Token + permission service** — issues, hashes, validates, and
  revokes per-agent tokens; evaluates scope against document
  `visibility`.
- **Audit log** — append-only, hash-chained record of every read and
  write (by whom, when, what, scope used).
- **Crypto** — key derivation, vault encryption (v2), recovery codes,
  signing.

Everything else lives in the TypeScript layer.

### 4.3 UI (TypeScript / React)

- Vault picker and onboarding wizard.
- Markdown editor with a frontmatter form (type-aware field hints).
- Graph view of `[[wikilinks]]`.
- Token manager: create, scope, revoke, last-used.
- Audit log viewer.
- Proposal review queue.
- In-app onboarding agent chat.

### 4.4 Local API (HTTPS on 127.0.0.1)

A small REST surface for agents and automations that can't speak MCP.
Same auth and scoping as MCP. Bound to loopback only. See
`docs/API.md` (not yet written) for the wire format.

### 4.5 MCP server

The primary agent surface. Tools exposed in v1:

- `context.search(query, scope?)` — full-text + semantic search,
  filtered by the calling token's scope.
- `context.get(id)` — fetch a single document.
- `context.list(type?, tags?)` — enumerate documents.
- `context.propose(id, patch)` — submit a change for user review.

Transport: stdio for local agents, HTTP/SSE for remote agents via the
cloud relay.

### 4.6 Mytex Server (Phase 2 — self-host or SaaS)

A single axum service (`mytex-server`) runs three deployment shapes
from one codebase (see `implementation-status.md` Phase 2 for the
detailed plan):

- **Personal synced** — the user's desktop and web clients read/write
  their own vault on the server.
- **Team self-host** — a business runs `mytex-server` on their own
  infra (published Docker image + reference `docker-compose.yml`).
  Members connect from desktop or web.
- **Team SaaS** — we operate the same image multi-tenant.

Shared concerns:

- **Storage.** Postgres for account/session/membership metadata and
  opaque blobs. The vault format (markdown + YAML frontmatter) is
  preserved; the server persists documents as encrypted blobs plus
  search index.
- **Decryption model.** Session-bound (per ARCH §3.4 and
  `reconciled-v1-plan.md` Q3): the server can decrypt only while a
  client device has published a short-lived session key. No client
  online past the TTL → blobs go opaque, hosted agents see a locked
  state.
- **Web client.** A Vite/React app that reuses `apps/desktop/src/`
  components and decrypts in-browser via a WASM-compiled
  `mytex-crypto`.
- **Agent access.** MCP over HTTP/SSE authenticated by OAuth 2.1
  bearer tokens (D4). Opaque tokens remain for loopback / stdio.

The entire server stack is open source; the commercial value is in
running it.

---

## 5. Security model

### 5.1 Data at rest

- v1: plain markdown files. OS-level file permissions only.
- v2: optional vault encryption (per-file envelope). OS keychain
  holds the derived key; passphrase required once per session.
- Cloud blobs: always encrypted, even in v1.

### 5.2 Agent authentication

- Each agent connection has its own opaque token.
- Server stores only a hash.
- Tokens carry: scope (which `visibility` labels it can read), mode
  (read or read+propose), expiry, retrieval limits (max documents
  and max tokens per request), and a human-readable label
  ("Claude — work laptop").
- One-click revoke. Last-used timestamp visible in the UI.
- Tokens never appear in logs or audit entries; the audit refers to
  token IDs.
- The token service is designed to accept an OAuth 2.1 path
  (PKCE, audience-bound tokens, protected-resource metadata) for
  the cloud-relay milestone. v1 local ships with opaque tokens only.

### 5.3 Scope evaluation

Every request is evaluated as:

```
allowed = token.mode covers request.action
       ∧ document.visibility ∈ token.scope
       ∧ token not expired
       ∧ token not revoked
```

Scope is the atom of permission. `visibility` values in v1:
`public`, `work`, `personal`, `private`, plus any custom labels the
user creates.

`private` is a **hard floor**: a token's scope must name `private`
explicitly to access any `private`-labelled document. The UI shows a
distinct warning when a user approves a grant that includes `private`.
Enumeration returns the same `not_authorized` error for
out-of-scope, nonexistent, and `private`-without-grant cases, so the
error cannot be used to detect `private` content.

### 5.4 Write surface

External agents can never write directly. `context.propose` lands in
`.mytex/proposals/` as a patch against a specific document version.
The desktop app shows a diff; user approves or rejects. The in-app
onboarding agent is the single exception, and only while the user is
watching.

### 5.5 Prompt injection

Context documents are user-controlled but can contain text copied from
untrusted sources. The core treats all document bodies as untrusted
input when rendering to agents: no special escape sequences, no
instruction-like phrasing is given elevated meaning.

Mytex attaches **provenance metadata** to every fragment it returns
(`document_id`, `visibility`, `updated_at`, `source`) and marks the
body as untrusted input. Agents can use that metadata to defend
themselves. Mytex does **not** rewrite, strip, or re-label
instruction-like content inside document bodies — sanitizing
user-authored markdown is fragile and paternalistic.

Retrieval-volume limits (max documents and max tokens per request,
configured per token) are enforced server-side as a
denial-of-exposure control.

### 5.6 Transport

- Local API and MCP HTTP bind to `127.0.0.1` only. No LAN exposure.
- Cloud relay uses mTLS between desktop and relay.
- Web UI served over HTTPS with HSTS and a strict CSP.

### 5.7 Audit

Every read and write, local or remote, produces an audit entry:
`(timestamp, token_id or "owner", action, document_id, scope_used,
outcome)`. The log is append-only and hash-chained, so tampering is
detectable. The UI can filter by token, document, or time range, and
export the log to the user on demand.

### 5.8 Recovery

- Cloud-synced vaults: recovery code shown once at setup, printable.
- Optional: Shamir-split recovery across N trusted contacts.
- No server-side recovery. This is a product feature, not a limitation.

### 5.9 Supply chain

- Reproducible builds for all desktop binaries.
- Signed releases.
- The Rust core's dependency set is kept deliberately small and
  pinned. Adding a dependency to the core requires explicit review.
- The TypeScript layer uses a locked, audited dependency set; updates
  go through Dependabot-style review.

---

## 6. Open source and commercial split

- **Open source (permissive license):** core engine, file format
  spec, desktop app, local API, MCP server, self-hosted sync server,
  web UI source.
- **Commercial (hosted):** operated cloud sync, operated MCP relay,
  priority support, and — later — team/org features (SSO, shared
  context, admin console, policy).

Openness of the format and protocol is load-bearing: the security
claims are only credible because anyone can verify them.

---

## 7. Workspaces, identity, and teams

v1 ships a single local vault. Phase 2 (`implementation-status.md`)
extends this along three axes without breaking the v1 contract:

- **Workspaces (Phase 2a).** A user's desktop app holds N workspaces,
  each with its own root, audit log, tokens, and index. A registry
  at `~/.mytex/workspaces.json` tracks them and the active one. The
  in-app switcher moves between them. No schema change to the vault.
- **Account + memberships (Phase 2b+).** A Mytex account is a single
  login (D8). A workspace can be local (today), remote-personal
  (Phase 2b), or remote-team (Phase 2c). Workspaces are independent
  — tokens, audit logs, and visibility labels do not cross.
- **Teams (Phase 2c).** A team workspace is a remote workspace with
  memberships. Three roles: `owner`, `admin`, `member` (D11). A seed
  `org/` type with an `org:` visibility holds business context
  (goals, tone, marketing stance) — admin-only write (D10); first
  user of a new team is admin automatically. Members may
  `context.propose` against `org/*`.

Design invariants:

- **Actor model.** Every document, token, and proposal has a
  `principal` field. v1 always uses the single user. Teams add new
  principals (team + members) without schema change.
- **Per-document visibility, not per-document ACLs.** Team roles map
  to default scope sets over `visibility` labels; no per-document
  ACL table.

v1 ships none of the Phase 2 UI or server. Nothing below the workspace
registry boundary is network-aware today.

---

## 8. v1 scope

In scope:

1. Tauri desktop app: create vault, browse and edit markdown with
   frontmatter, graph view.
2. Seed context types: identity, roles, goals, relationships,
   memories, tools, preferences, domains, decisions (see
   `FORMAT.md`).
3. Local MCP server with `context.search`, `context.get`,
   `context.list`. Compatibility targets: Claude Desktop (primary),
   Cursor (guaranteed secondary — required pass in the v1 test matrix).
4. Scoped token management UI and audit log viewer.
5. Per-document passive history UI (timeline, diff, restore). No
   branches, no remotes — the product's job is to be simpler than
   git, not to be a git client. Users remain free to run `git` on
   the vault from outside the app.
6. Import from Obsidian vault.
7. In-app onboarding agent flow.

Out of scope for v1:

- Cloud sync and relay.
- REST API (can ship in v1.1 if needed).
- `context.propose` write-back flow.
- Any team, org, or RBAC surface.
- Mobile apps.
- Browser extension.

---

## 9. Phase 2 roadmap

Phase 2 is tracked in detail in `docs/implementation-status.md`. The
short shape:

| Phase | Delivers                                                | New crates                     |
| ---   | ---                                                     | ---                            |
| 2a    | Multi-vault desktop + workspace switcher                | —                              |
| 2b    | `mytex-server` + remote driver + web client + sync      | `mytex-server`, `mytex-sync`, `mytex-crypto`, `apps/web` |
| 2c    | Teams, memberships, `org/` context, roles               | —                              |

Phase 2 decisions (D7–D12) are recorded in `implementation-status.md`
and supersede anything in `reconciled-v1-plan.md` that conflicts.
`reconciled-v1-plan.md` stays scoped to v1 (D1–D6).

---

## 10. Glossary

- **Vault** — a directory of context files. One per workspace.
- **Workspace** — a registered vault the client can switch to.
  Personal (local) or team (remote) in Phase 2+.
- **Account** — a Mytex login; may belong to N workspaces.
- **Document** — a single markdown file in a vault.
- **Type** — a document's top-level category (`identity`, `goal`,
  …, `org` in Phase 2c).
- **Visibility** — a label on a document used for permission scoping.
  `org:` is added in Phase 2c.
- **Token** — an opaque credential granting a specific agent a
  specific scope within one workspace.
- **Principal** — the owner of a vault, token, or document. The
  single user in v1; a user or team in Phase 2+.
- **Scope** — the set of `visibility` labels a token may read.
- **Proposal** — an agent-submitted change awaiting user approval.
- **Role** (Phase 2c) — `owner` / `admin` / `member` within a team
  workspace. Roles translate to default scopes.
