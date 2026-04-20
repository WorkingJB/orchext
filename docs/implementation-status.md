# Mytex — Implementation Status

Running status of the v1 build. Updated after each crate or significant
milestone. Other docs describe *intent* (`ARCHITECTURE.md`, `FORMAT.md`,
`MCP.md`, `reconciled-v1-plan.md`); this one describes *state*.

A new session should be able to open this file and know exactly where
we are without reading git history.

---

## Snapshot

**Last updated:** 2026-04-19

**Toolchain:** Rust 1.95.0 stable (rustup). Workspace at repo root.

**Test totals:** 118/118 passing with `DATABASE_URL` set (for the
`mytex-server` integration tests against a live Postgres); 109/109
without. +23 tests for Phase 2b.1 (14 unit, 9 integration).

| Crate         | Status        | Unit | Integration | Notes                                  |
|---------------|---------------|-----:|------------:|----------------------------------------|
| `mytex-vault` | ✅ shipped     | 12   | 6           | Format parser + `PlainFileDriver`      |
| `mytex-audit` | ✅ shipped     | 2    | 5           | Hash-chained JSONL log                 |
| `mytex-auth`  | ✅ shipped     | 11   | 9           | Opaque tokens + Argon2id + scopes      |
| `mytex-index` | ✅ shipped     | 4    | 6           | SQLite + FTS5; search / graph / filter |
| `mytex-mcp`   | ✅ shipped     | 11   | 22          | JSON-RPC + stdio; rate limit + fs watcher |
| `mytex-desktop` | ✅ Phase 2a  | 7    | —           | Multi-vault switcher + workspace registry |
| `mytex-server`| ✅ Phase 2b.1 | 14   | 9           | Axum + Postgres + opaque sessions      |

---

## Shipped crates (details)

### `mytex-vault` — 2026-04-18

The vault format parser and storage driver abstraction.

**Public API:**

- `Document` — parse / serialize / version (SHA-256)
- `Frontmatter` — all seed fields + `extras` (BTreeMap) preserves unknown/x-* fields round-trip
- `DocumentId` — newtype validated per `FORMAT.md` §3.3
- `Visibility` — `Public | Work | Personal | Private | Custom(String)`; `is_private()` only true for the built-in `Private`
- `VaultDriver` — async trait: `list`, `read`, `write`, `delete`
- `PlainFileDriver` — disk-backed impl, skips `.mytex/` and dot-dirs
- `VaultError` — `thiserror` enum

**Notable tests:**

- Round-trip preserves `x-*` extensions (FORMAT.md §3.4 commitment)
- `private` hard floor: built-in `Private` reports `is_private()` true; `Custom("semi-private")` does not
- `PlainFileDriver` rejects `write(id, doc)` when `id` doesn't match `doc.frontmatter.id`
- `.mytex/` directory is skipped by `list()`

**Decisions recorded here:** none — matches spec.

### `mytex-audit` — 2026-04-18

Append-only, hash-chained JSONL audit log. Matches `ARCHITECTURE.md` §5.7 and `MCP.md` §9.

**Public API:**

- `AuditWriter::open(path)` — recovers chain state (seq, last hash) from existing file
- `AuditWriter::append(AuditRecord) -> AuditEntry` — atomic append (O_APPEND + flush), rotates state
- `verify(path) -> VerifyReport` — rehashes every entry, fails at the exact `seq` where the chain breaks
- `Iter` — stream entries from disk
- `Actor::{Owner, Token(String)}` — serializes as `"owner"` or `"tok:<id>"` (literal string, not JSON object)
- `Outcome::{Ok, Denied, Error}`

**Decisions recorded here:**

- **JSONL not SQLite.** Log file is newline-delimited JSON; chosen over a SQLite table for append simplicity, grep-ability, and so the log survives even if SQLite schemas drift. The indexer (below) is what uses SQLite.
- **Hash input is compact JSON of a fixed-field struct.** Deterministic because field order is declaration order in a struct (not a map).
- **Canonical hash excludes the `hash` field** of the entry itself (chicken-and-egg), but includes `prev_hash`, so tampering with any field is detected.

**Notable tests:**

- Reopen preserves chain: writer close + reopen + append continues at the right seq with the right `prev_hash`
- Tamper detection identifies the specific seq where the chain broke
- Empty log verifies cleanly (0 entries, no last seq/hash)

### `mytex-auth` — 2026-04-18

Token service: issuance, Argon2id hashing, scope eval including the `private` hard floor, revocation, expiry, retrieval limits.

**Public API:**

- `TokenService::open(path)` — loads `tokens.json` or starts empty
- `TokenService::issue(IssueRequest) -> IssuedToken` — returns secret + public info
- `TokenService::authenticate(&str) -> AuthenticatedToken` — constant-time-ish verify via Argon2id
- `TokenService::revoke(id)`, `mark_used(id, ts)`, `list()`
- `Scope` — `BTreeSet<String>` wrapper with `allows_label`, `allows(&Visibility)`, `includes_private`, `narrow_to(&[String])`
- `Mode::{Read, ReadPropose}`
- `Limits { max_docs: u32, max_bytes: u64 }` — default 20 docs / 64 KiB per `MCP.md` §3.1
- `TokenSecret` — Debug-redacted newtype (never prints the raw value)
- `IssueRequest`, `IssuedToken`, `AuthenticatedToken`, `PublicTokenInfo`

**Decisions recorded here:**

- **Secret format: `mtx_` + base64url-no-pad of 32 random bytes.** Matches `MCP.md` §3.1 intent; 43-char payload, url-safe for stdio copy-paste.
- **Token ID: `tok_` + base64url-no-pad of 12 random bytes.** Separate from the secret, goes in audit logs, never leaks secret bits.
- **Atomic persistence via write-temp + rename.** Prevents torn JSON files under crash.
- **`Scope::narrow_to` is intersection-only.** Can never widen — matches `MCP.md` §3.2.
- **Private hard-floor is enforced by construction.** `Scope::allows_label` is a literal-string match against the scope set; no implicit promotion anywhere. Tests cover: token without `"private"` can't read `Private` docs; custom `semi-private` label doesn't accidentally grant `Private` access.

**Notable tests:**

- Issue → authenticate roundtrip
- Wrong secret / malformed secret / revoked / expired all reject with distinct errors
- `PublicTokenInfo` serialization never emits the hash
- Persists across reopen (tokens file survives service drop)
- Private floor enforced both ways (denies without `private`, allows with `private`)

---

### `mytex-index` — 2026-04-18

Full-text search + tag/type filter + link graph over the vault. Backed
by SQLite with FTS5.

**Public API:**

- `Index::open(path)` — opens or creates `index.sqlite` at the given path; applies schema idempotently
- `Index::reindex_from(&dyn VaultDriver) -> IndexStats` — full rebuild from a vault; the contract that makes `index.sqlite` safely deletable (FORMAT.md §7)
- `Index::upsert(type_, &Document)` — insert or replace a document plus its tags, links, and FTS row
- `Index::remove(&DocumentId)` — drops from all tables including FTS
- `Index::search(SearchQuery) -> Vec<SearchHit>` — FTS5 bm25-scored, filtered by type/tag/visibility/updated_since, with snippet
- `Index::list(ListFilter) -> Vec<ListItem>` — enumerate, same filters, no body
- `Index::backlinks(id)` / `outbound_links(id)` — graph queries

**Decisions recorded here:**

- **rusqlite with `bundled` feature.** No system SQLite dependency; binary is self-contained. FTS5 is compiled in.
- **Async wrapper via `tokio::task::spawn_blocking`.** rusqlite is synchronous; `Arc<Mutex<Connection>>` (std mutex, since we're in blocking context) serializes access within a process.
- **Contentful FTS5 table, not external-content.** Slight storage duplication (body is in both `documents` and `search`); huge simplicity win — no triggers, straightforward upsert.
- **`documents` + `tags` + `links` normalized.** `ON DELETE CASCADE` drops tags and links when a document is removed; FTS row is dropped explicitly.
- **Scope filtering is an `IN` clause on `visibility`.** Passing `allowed_visibility` is how callers apply the `private` hard floor: if `"private"` isn't in the set, no `private` documents surface. Consistent with how `mytex-auth` thinks about scope.
- **Title extraction is `# Heading` → first non-empty H1, fallback to `id`.** Matches MCP.md §5.1.
- **`WAL` journal mode enabled.** Better concurrency (the desktop UI might read while MCP writes), negligible cost.

**Notable tests:**

- `search_respects_scope_filter_and_private_floor`: proves a scope without `"private"` cannot surface `Visibility::Private` documents, even when the query matches the body.
- `remove_drops_from_all_tables_including_fts`: after remove, search misses, backlinks/outbound disappear, list excludes it.
- `upsert_replaces_tags_and_links`: re-upserting a document replaces (not unions) its tag and link sets.
- `reindex_from_vault_and_search`: reindex produces correct `IndexStats`, subsequent search returns hits.

### `mytex-mcp` — 2026-04-19

JSON-RPC 2.0 MCP server over stdio. Wires the four backing services
(`vault`, `index`, `auth`, `audit`) behind the v1 surface defined by
`MCP.md`.

**Public API (library):**

- `Server::new(vault, index, auth, audit, token)` — one server per
  connection; `token` is an `AuthenticatedToken` already verified.
- `Server::handle(Request) -> Option<Response>` — dispatches one
  JSON-RPC message. Returns `None` for notifications.
- `McpError` / `McpError::to_rpc()` — the code/tag mapping from
  `MCP.md` §7 (`-32000..-32007`).
- `rpc::{Request, Response, Notification, RpcError, Id}` — wire
  envelope types.

**Binary:** `mytex-mcp --token <TOKEN> --vault <VAULT_DIR>`. Reads
line-delimited JSON from stdin, writes line-delimited JSON to stdout.

**Implemented methods:** `initialize`, `initialized` (notification),
`ping`, `tools/list`, `tools/call`, `resources/list`, `resources/read`,
`resources/subscribe`, `resources/unsubscribe`.

**Tools:** `context.search`, `context.get`, `context.list` under
the `context.` namespace (D3). Results include provenance
(`visibility`, `updated`, `source` when set).

**Decisions recorded here:**

- **Token pre-authenticated at startup.** `main.rs` calls
  `TokenService::authenticate` before reading a single byte of
  JSON-RPC input. An invalid token exits non-zero immediately;
  every JSON-RPC message after that is implicitly authorized as
  the pre-verified principal. This matches MCP.md §2.1 (stdio
  launch) where the token arrives via `--token` and is bound to
  the process lifetime.
- **Index is rebuilt from the vault on every `serve` start.**
  `reindex_from` is idempotent and cheap at v1 vault sizes. This
  guarantees the index matches disk at T0 — important because the
  fs watcher only fires on changes *after* it starts, so any docs
  added while the server was down would otherwise be invisible
  until touched.
- **Rate limit: 60 requests / 10-second sliding window per token.**
  Applies to `tools/*`, `resources/*`. `initialize`, `ping`, and
  notifications are exempt — the limiter protects the indexer
  and fs, not handshakes. When saturated returns `-32005 /
  rate_limited` with `error.data.retry_after_ms` set to the wait
  until the oldest in-window request ages out.
- **`not_authorized` is deliberately ambiguous.** Out-of-scope,
  nonexistent, and private-without-private-scope documents all
  return `-32002 / not_authorized` from `context.get` and
  `resources/read`. A test (`get_nonexistent_is_indistinguishable_from_out_of_scope`)
  pins this so it cannot regress.
- **Private hard floor is re-checked defensively in `context.get`.**
  The index layer already enforces it via `allowed_visibility`, but
  `get` reads from the vault (not the index) and re-checks
  `visibility.is_private() && !scope.includes_private()` so a
  future refactor of `Scope::allows` cannot silently widen access.
- **`scope` request argument narrows only, never widens.**
  `Scope::narrow_to` is intersection; a `scope: ["private"]`
  argument on a token without `"private"` errors out rather than
  granting access. Returned as `-32004 / invalid_argument`.
- **Provenance-only, no sanitization (D5).** Results carry the
  frontmatter `source` when set. The server does not scrub,
  relabel, or reinterpret body text. For search hits `source`
  costs one extra `vault.read` per hit — acceptable at the
  bounded limits (≤100 docs); re-evaluate if needed by promoting
  `source` into the index schema.
- **Retrieval limits enforced in order `hard cap → token cap →
  request`.** `limit` is clamped to 100 (hard), then to
  `token.limits.max_docs`, then to what the caller asked for.
  For search, a running `max_bytes` counter over snippet bytes
  can truncate early and set `truncated: true`. For `context.get`,
  `max_bytes` is not applied — a single-document fetch that the
  caller asked for by ID should not be silently truncated.
- **`resources/subscribe` emits updates via an fs watcher.** The
  `notify` crate watches the vault root recursively (fsevent backend
  on macOS; default elsewhere). On Create/Modify/Remove the watcher
  thread classifies the path as `(type, id)`, upserts or removes the
  doc from the index, then calls `Server::emit_resource_updated`
  which fires `notifications/resources/updated` if the URI matches
  a subscription (exact, type-prefix, or root). The vault root is
  canonicalized at startup so fsevent's absolute paths line up with
  the driver root.
- **Audit on every dispatched call.** Every
  `context.*` / `resources.read` call appends one JSONL entry
  with actor = `tok:<id>`, outcome `ok` or `denied`, and the
  scope in effect. `auth.mark_used` is touched on every attempt
  (including denials) so revoked tokens still leave a trail.
  Audit-write failure is logged via `tracing::warn` but never
  fails the caller — the user's read must succeed even if the
  audit sink is wedged.
- **`tools/call` returns both `content` (text) and
  `structuredContent` (typed JSON).** MCP clients that only look
  at `content` get the tool result as a stringified JSON block;
  strict clients read `structuredContent` directly without a
  second parse.
- **Tool input validation is hand-rolled (serde + explicit
  length checks).** No JSON-schema validator dep. `tools/list`
  still advertises schemas so agents can self-validate before
  calling.

**Notable tests:**

- `search_private_floor_requires_explicit_private`: a token
  without `private` cannot surface a private diary entry even
  when the query body matches; with `private` in scope, it does.
- `search_rejects_widening_scope_argument`: a `scope: ["private"]`
  request on a work-only token returns `-32004 / invalid_argument`,
  not a widened result set.
- `get_nonexistent_is_indistinguishable_from_out_of_scope`:
  both map to `-32002 / not_authorized` (enumeration defence).
- `resources_list_filters_by_scope`: resource listings omit
  URIs the token can't read; direct `resources/read` to those
  URIs returns `-32002`.
- `audit_log_grows_per_call`: both an ok `context.list` and a
  denied `context.get` append chained JSONL entries that
  `mytex_audit::verify` accepts.

**Binary subcommands:**

- `mytex-mcp init --vault <DIR> [--label <L>] [--scope work,public]
  [--ttl-days N]` — creates the vault skeleton (seed type dirs +
  `.mytex/`), issues an initial token, and prints (a) the token
  secret (shown once), (b) the launch command, (c) a
  ready-to-paste Claude Desktop `mcpServers` entry.
- `mytex-mcp serve --vault <DIR> --token <TOKEN>` — the JSON-RPC
  server itself. Reindexes at startup, spawns the fs watcher,
  then enters a `tokio::select!` loop over `(stdin lines,
  notification channel)`. On stdin EOF it drains any in-flight
  notifications for up to 250 ms before exiting, so an fs event
  racing a disconnect still reaches the client.

**Known gaps (not in v1 surface):**

- `context.propose` returns method-not-found; intentionally
  deferred to v1.1 per MCP.md §5.4 and reconciled-v1-plan D6 (it
  depends on the desktop review UI).
- FSEvents coalesces bursts; a single `echo >> file.md` can emit
  2–3 `notifications/resources/updated` for one logical write.
  Clients dedupe by URI; this is a minor politeness issue, not a
  correctness one. Debouncing would require `notify-debouncer-mini`
  and is deferred.

---

### `mytex-desktop` — 2026-04-19

Tauri 2 desktop app (Rust backend + React/Vite/TS/Tailwind frontend).
Lives at `apps/desktop/`; the Rust side is `apps/desktop/src-tauri/`
(workspace member `mytex-desktop`) and the frontend at
`apps/desktop/src/`.

**Screens:**

- **Vault picker** (first run or "Switch vault"): directory dialog via
  `tauri-plugin-dialog`; `vault_open` creates the seed type dirs +
  `.mytex/`, opens the persistent stores, runs a full `reindex_from`,
  and returns a `VaultInfo` snapshot.
- **Documents**: three-pane layout — types sidebar, document list,
  detail editor. New / save / delete with frontmatter fields (id,
  type, visibility, tags, source) and a markdown body textarea.
  Every save goes through `vault.write` then `index.upsert` so search
  stays consistent.
- **Tokens**: list active + revoked tokens; issue form (label, scope
  checkboxes with a distinct `private` warning, TTL days); the secret
  is shown exactly once in a dismissable panel, then only the
  redacted `PublicTokenInfo` remains on screen.
- **Audit**: reverse-chronological table of `AuditEntry` rows with a
  "chain verified" / "chain broken" badge backed by
  `mytex_audit::verify`.

**Commands (Tauri backend):** `vault_open`, `vault_info`, `doc_list`,
`doc_read`, `doc_write`, `doc_delete`, `token_list`, `token_issue`,
`token_revoke`, `audit_list`. All are `async` and call the existing
crates directly — no subprocess to `mytex-mcp`.

**Decisions recorded here:**

- **Services managed as `tokio::sync::RwLock<Option<OpenVault>>`** in
  Tauri state. Commands `clone` out a `Services` snapshot of `Arc`s
  under a short read lock, then do their work without holding the
  lock, so concurrent requests don't serialize behind a slow command.
- **Frontend calls crates through Tauri commands, not an in-process
  MCP server.** An alternative was to embed `mytex-mcp` and talk to
  it over stdio internally. Direct calls are simpler, keep the MCP
  surface authoritative for agents (who are external by definition),
  and avoid re-serializing every payload through JSON-RPC twice.
- **Secret is shown once, then only `PublicTokenInfo`.** The
  `token_issue` command returns `{ info, secret }`; the UI renders
  the secret in a yellow dismissable panel with a copy button.
  After dismiss, `token_list` no longer has access to the secret
  (it was never stored in plaintext — Argon2id hash only).
- **Reindex on vault open.** Same argument as mytex-mcp: watcher
  (not yet wired in the desktop — see below) only fires on changes
  *after* it starts, so any docs edited outside the app need a
  ground-truth rebuild to surface in list/search.
- **Markdown body is a `<textarea>`, not a rich editor.** Scope cut.
  CodeMirror / rich preview is worth adding later but would have
  tripled the UI work for little gain at this stage.
- **Tailwind 3.4 + hand-rolled components** over shadcn/MUI/etc.
  One style system, no transitive design-system churn; easy to
  rip out if we pick a component library later.
- **Icon is a generated placeholder.** `icons/icon.png` is a 32x32
  solid-purple PNG produced from a Python one-liner; replace before
  any distribution build.

**Binary workflows:**

- **Dev:** `cd apps/desktop && npm run tauri dev` — vite on
  `localhost:1420`, Rust hot-reload from `src-tauri/`. Requires
  `~/.cargo/bin` on PATH (Tauri invokes `cargo metadata` at startup).
- **Build:** `npm run tauri build` — full `.app` / `.dmg` bundle.
  Not exercised yet; icon needs replacement first.

**Follow-ons shipped since MVP (2026-04-19):**

- **Fs watcher wired** — `src-tauri/src/watch.rs` mirrors the
  `mytex-mcp` pattern: notify watcher owns path→(type,id), calls
  `index.upsert`/`remove`, emits Tauri event `vault://changed`.
  `DocumentsView` and `GraphView` subscribe and auto-refresh. No
  debouncing; bursts may trigger several events per logical write.
- **Save indicator** — `DocEditor` flashes a transient "Saved ✓"
  pill for ~1.8s on success and persists a red error banner on
  failure. `role="status" aria-live="polite"` for assistive tech.
- **Graph view** (reconciled-v1-plan §v1 item 1) — new `Graph`
  nav tab. Backend: `graph_snapshot` Tauri command + a new
  `Index::all_edges()` that pulls every `(source, target)` link
  row in one SQL trip. Frontend: `react-force-graph-2d` canvas,
  click-to-jump to Documents. Orphan edges (target not in vault)
  are filtered out — this is a v1 simplification, not a bug.
- **In-app onboarding agent** (§v1 item 6) — first-run screen
  (auto-opened when `document_count == 0`, also a nav tab).
  Chat UI backed by `onboarding_chat` / `onboarding_finalize`
  Tauri commands that POST directly to Anthropic's `/v1/messages`
  endpoint via `reqwest` (no Rust SDK exists). Model pinned to
  `claude-haiku-4-5` for cost. Scope cuts: no streaming, no tool
  use (agent returns a JSON array of seed docs in a finalize turn),
  single-session only. API key stored in `.mytex/settings.json`
  alongside `tokens.json` — plaintext at rest, same threat model
  as the existing token file, move to OS keychain in a follow-up.

**Known gaps remaining:**

- **Obsidian import** (§v1 item 5) — explicitly cut from the MVP;
  not started.
- **API key in plaintext** — `.mytex/settings.json` is not
  encrypted. Fine for local dev, but should move to
  `tauri-plugin-stronghold` / OS keychain before any distribution
  build.
- **Fs watcher burst coalescing** — a single `echo >> file.md`
  can emit 2–3 `vault://changed` events. Harmless (React just
  re-fetches), but noisy; `notify-debouncer-mini` would smooth it.

**Phase 2a shipped (2026-04-19): Multi-vault + workspace switcher**

The desktop app now tracks N registered vaults and switches between
them from the header. Unblocks use case 5 locally (personal ↔ any
other local vault).

- **Registry at `~/.mytex/workspaces.json`** — per-install client
  state (not part of the vault format; see `FORMAT.md` §11.1). JSON
  with `{version, active, workspaces:[{id, name, kind, path,
  added_at}]}`. Atomic write via temp + rename. Workspace IDs are
  `ws_` + base64url of 8 random bytes (matches `tok_` pattern).
- **New Rust module:** `apps/desktop/src-tauri/src/workspaces.rs`
  (Registry + WorkspaceEntry + helpers) with 4 unit tests
  (empty-load, roundtrip, path-dedup, active-promotion on remove).
- **State model:** `AppState { registry_path, registry: RwLock<Registry>,
  open: RwLock<Option<OpenVault>> }`. Only the active workspace is
  open at a time; switching drops the old `OpenVault` (and its
  watcher) before opening the new one. Deliberate simplification:
  keeping N warm would require N watchers + coordinating the fs-event
  channel, and v1 vault sizes don't need it.
- **New commands:** `workspace_list`, `workspace_add(path, name?)`,
  `workspace_activate(id)`, `workspace_remove(id)`,
  `workspace_rename(id, name)`. `vault_open` is gone; frontend
  calls `workspace_add` instead.
- **`vault_info()` now auto-opens** the active registered workspace
  if present but not loaded. Returns `null` only on first run
  (registry empty). Existing `doc_*` / `token_*` / `audit_*`
  commands route through `active_services()`, which returns a clear
  "no workspace open" error if called before any workspace is
  registered.
- **`VaultInfo` grew** `workspace_id` and `name` fields so the
  frontend can key React children off the active workspace.
- **UI:** new `WorkspaceSwitcher.tsx` dropdown in the header showing
  active + list + "Add workspace…" + per-row Rename / Remove.
  Remove on the last remaining workspace is refused at the UI layer
  (the backend would simply leave an empty registry with no active).
- **Re-mount on switch:** `Layout.tsx`'s `<main>` carries
  `key={vault.workspace_id}`, so all child views (Documents, Graph,
  Tokens, Audit, Onboarding) unmount + remount on switch and re-
  fetch cleanly. Avoided threading a workspace prop through every
  child; React keying is the lighter touch.
- **Workspace isolation** is path-based (same as v1): each vault's
  `.mytex/` holds its own tokens, audit, index, proposals, settings.
  No cross-workspace data paths added.

**Decisions recorded here:**

- **Single-open, not multi-open.** As above; revisit only if
  workspace count grows past ~10 or users ask for cross-vault search.
- **Registry outside the vault, not inside.** Vault portability
  wins. A vault dropped onto another machine registers as a new
  workspace on that machine without rewriting anything inside it.
- **No React Router.** Workspace is React state in `App.tsx`, not
  a URL path segment. URL-based routing (`/w/:id/...`) was in the
  Phase 2a plan but was cut — it adds a dependency and deep-link
  semantics we don't yet need.
- **Rename is admin-free.** Users can rename any registered
  workspace from the switcher; no confirmation. Revisit if a
  workspace name ever appears in audit logs or tokens (it doesn't
  yet).

**Known gaps after Phase 2a:**

- **UI not exercised in a browser.** Code type-checks and
  `vite build` succeeds; interactive smoke-test deferred to the
  user or next session (Tauri dev needs the native shell).
- **Fs watcher thrash on rapid switches.** Switching workspaces
  tears down and recreates the watcher. Heavy clicking could
  produce brief gaps where file changes on the previous workspace
  would be missed; that workspace isn't active, so no user-visible
  effect. The next reindex on reactivation catches up.
- **No keyboard shortcut** for workspace switching yet.

### `mytex-server` — 2026-04-19 (Phase 2b.1)

Axum HTTP server, Postgres-backed account + session store. Proves the
deployment shape (Docker, Postgres, migrations) before vault endpoints,
crypto, or MCP HTTP depend on it. No vault endpoints yet — those are
2b.2.

**Public API (library):**

- `router(state: AppState) -> axum::Router` — full app router,
  including `/healthz` and `/v1/auth/*`.
- `migrate(&PgPool)` — runs embedded `./migrations` against the pool.
- `AppState { db, sessions }` — handle shared across handlers.
- `sessions::SessionService` — `issue / authenticate / revoke /
  list_for_account`.
- `accounts::{signup, by_id, verify_password}` — account CRUD with
  argon2 password hashing and email normalization.
- `password::{hash, verify}` — thin Argon2id wrapper (PHC strings).

**Binary:** `mytex-server`. Reads `DATABASE_URL` and optional
`MYTEX_BIND` (default `0.0.0.0:8080`); runs migrations on startup,
serves traffic, shuts down cleanly on SIGINT/SIGTERM.

**Routes:**

- `GET  /healthz` — `{"ok": true}`, no auth.
- `POST /v1/auth/signup` — email + password; creates account +
  personal tenant + owner membership + first session.
- `POST /v1/auth/login` — returns a new session for valid creds.
- `GET  /v1/auth/me` — authenticated; returns current account.
- `GET  /v1/auth/sessions` — authenticated; lists caller's
  non-revoked sessions.
- `DELETE /v1/auth/logout` — authenticated; revokes the current
  session.

**Schema (`migrations/0001_initial.sql`):**

- `accounts(id, email, password, display_name, created_at, updated_at)`
  with a `lower(email)` index.
- `sessions(id, account_id, token_prefix, token_hash, label,
  created_at, expires_at, last_used_at, revoked_at)` — opaque
  `mtx_*` secret, Argon2id-hashed at rest, first-12 prefix indexed.
- `tenants(id, name, kind)` — `kind in {personal, team}`, one
  personal tenant auto-created per account.
- `memberships(tenant_id, account_id, role)` — `role in {owner,
  admin, member}`; currently only the owner row is created at signup.
  Unused beyond bootstrap until Phase 2c.

**Decisions recorded here:**

- **Runtime-checked queries, not `sqlx::query!` macros.** The macros
  need a live DB at compile time (or a prepared `.sqlx/` cache).
  Neither is set up yet. Using `sqlx::query_as::<_, StructWithFromRow>`
  gives us runtime-checked queries without compile-time infra. Tests
  (integration, against real Postgres) catch query errors. Migrate
  to `query!` once CI has Postgres + we run `cargo sqlx prepare`.
- **Enumeration-resistant auth errors.** Unknown email, wrong
  password, revoked session, expired session all map to the same
  `401 Unauthorized` with `error.tag: "unauthorized"`. The unknown-
  email branch of `verify_password` runs a dummy Argon2 verify
  against a fixed valid PHC string to keep response time roughly
  constant. A unit test (`accounts::tests::dummy_phc_parses`) pins
  the dummy so a future Argon2 upgrade can't break it silently.
- **Signup always issues a session.** A new user should not have to
  POST login immediately after signup. The signup response body
  matches the login response shape; clients treat them identically.
- **Personal tenant bootstrap is in the signup transaction.** If the
  tenant / membership insert fails, the account insert rolls back.
  Guarantees the invariant "every account has exactly one personal
  tenant they own" even under partial failure.
- **In-memory session cache, 60s TTL.** Every request hits the DB
  for session validation by default — cached for 60s after a
  successful lookup. Revocation invalidates the cache by session_id
  so `revoke → subsequent request` is immediately rejected (the test
  `logout_revokes_session` pins this). 60s is a deliberate staleness
  budget: an expired or password-changed session stays live up to
  60s after the change, which is fine for the product's threat
  model and saves one Argon2 verify per request. Cache is bounded
  at 10k entries (drops all on overflow — unsophisticated eviction
  is fine at this scale).
- **Session prefix of 12 chars** (`mtx_` + 8) for the lookup. Same
  pattern as `mytex-auth`: enough entropy in the prefix that there
  is effectively no collision risk in a single-tenant DB, indexed
  for O(1) lookup. The real verify is Argon2id against the stored
  hash.
- **`tenant_id` columns live now even though multi-tenancy isn't
  enforced** (2c). Avoids a future schema migration at the moment
  enforcement lands.
- **Runtime-only config from env.** No TOML / YAML config file. Two
  required vars (`DATABASE_URL`, optional `MYTEX_BIND`); anything
  else needs a code change. Keeps the deploy story tight.

**Notable tests:**

- `signup_then_me_roundtrip` — signup returns a bearer that lets
  `/v1/auth/me` return the same account.
- `login_unknown_email_indistinguishable_from_wrong_password` — pins
  the enumeration-resistance invariant. Both branches return 401
  with `error.tag: "unauthorized"`.
- `logout_revokes_session` — after DELETE /v1/auth/logout, the same
  bearer is rejected on the next request (cache invalidation works).
- `duplicate_signup_conflicts` — 409 on email re-registration,
  mapped from Postgres `23505 unique_violation`.
- `short_password_rejected` — 400 if password under 12 chars.
- `healthz_ok` — `/healthz` serves without auth.
- Plus 14 unit tests across `accounts`, `sessions`, `password`,
  `auth::bearer_from_headers`.

**Packaging:**

- `crates/mytex-server/Dockerfile` — multi-stage (rust-slim builder
  → debian-slim runtime). Runs as unprivileged user `mytex` uid 1000.
  No curl/wget baked in; healthcheck is compose's responsibility.
- `crates/mytex-server/docker-compose.yml` — spins up `postgres:16-alpine`
  + the server image built from the repo root. Dev uses
  `localhost:8080` over plain HTTP; production expects a TLS
  terminator in front.
- `crates/mytex-server/.env.example` — documented env vars
  (`MYTEX_POSTGRES_PASSWORD`). Not committed as `.env`.

**Known gaps after Phase 2b.1:**

- **No vault endpoints.** That's 2b.2. Today's server only does auth.
- **No email verification / password reset / rate limiting.** All
  additive; not in 2b.1's tight scope.
- **No CI Postgres.** Integration tests run locally against a
  docker-run'd Postgres (`docker run --rm -d -e POSTGRES_USER=mytex
  -e POSTGRES_PASSWORD=testpw -e POSTGRES_DB=mytex_test -p 5555:5432
  postgres:16-alpine` + `DATABASE_URL=postgres://mytex:testpw@
  localhost:5555/mytex_test`). Wiring the same into CI is a follow-
  up; currently a dev must have Docker to run the integration suite.
- **`sqlx::query!` macro migration.** Deferred until CI can run
  `cargo sqlx prepare` against a live DB and commit the `.sqlx/`
  cache. Until then, query errors surface only at runtime (caught
  by integration tests).
- **No MCP transport yet.** HTTP/SSE MCP lands with 2b.4.
- **No TLS in the reference compose file.** Plain HTTP on `:8080`.
  Production deployments add Caddy/Traefik/Nginx in front; we ship
  compose snippets for those when we publish the first image.

## Phase 2 — Multi-vault, server, teams

> Status: **in progress.** Phase 2a (multi-vault desktop) and
> Phase 2b.1 (server skeleton + auth) have shipped; see "Shipped
> crates" above for details. This section captures the remaining
> shape and decisions so any working session can pick up without
> re-deriving context.

### Goals — six use cases

1. **Personal self-host.** Desktop app + local MCP (shipped today).
2. **Personal synced.** One user, desktop + web client, context synced
   between devices.
3. **Team self-host.** Business customer runs `mytex-server` on their
   own infra; team members connect from desktop or web. Shared
   org-level context (marketing stance, goals, tone) alongside
   each member's personal workspace.
4. **Team SaaS.** Managed multi-tenant hosting of (3). Same artifact.
5. **Account + N memberships.** A single account can belong to
   personal + any number of teams; the client switches between
   workspaces (Slack/Linear model).
6. **Agent-led code updates.** Keep crates small and independently
   testable; `implementation-status.md` is the durable handoff so
   agents can pick up mid-stream.

### Deployment matrix

|              | Self-hosted                | Managed SaaS            |
| ---          | ---                        | ---                     |
| **Personal** | Desktop-only (today's v1)  | Desktop + web, synced   |
| **Team**     | On-prem `mytex-server`     | Hosted tenant of same   |

**Key claim:** one server artifact (`mytex-server`, axum) covers the
three non-trivial cells. SaaS is "we operate it for you" — no code
fork. Already promised by `ARCHITECTURE.md` §6.

### Architectural decisions (Phase 2)

**D7. Server packaging — Docker image + `docker-compose.yml`.**
On-prem customers get a published image plus a reference compose file
(server + Postgres + TLS-terminating reverse proxy). Lets them deploy
without owning an OS or dependency stack. The SaaS tenant runs the
same image. A signed standalone binary + systemd unit is possible
later but not first.

**D8. Identity — one account, N memberships.**
A Mytex account is a single login that can belong to any number of
workspaces (one personal + N teams). Client switches workspaces
in-app. Per-workspace tokens, audit logs, scopes, and visibility
labels stay isolated; an account is just the identity envelope.

**D9. Sync / decryption — session-bound, default on.**
`reconciled-v1-plan.md` Q3 cashes in. While any client is online and
unlocked, the server holds a short-lived session key and decrypts
server-side for hosted agents. When no client is online past the TTL,
the server falls back to opaque blobs and hosted integrations see a
locked state. Strict-E2EE opt-out available; those users' hosted
agents fall back to relay-to-device.

**D10. Org context — admin-write, first user is admin.**
Team workspaces get a seed `org/` top-level type. Only admins/owners
can write to `org/*`. The first user of a new team is made admin
automatically. Members read `org/*` subject to visibility. Members
with `read+propose` can submit `context.propose` patches for admin
review — the long-deferred propose flow finally earns its keep.

**D11. Team roles — three levels, mapped to scope.**
`owner` (billing + member management + org write), `admin` (member
management + org write), `member` (read + propose). Roles translate
to default scope sets; per-workspace tokens may narrow further.
No per-document ACLs.

**D12. No CRDTs.** Server is source of truth in sync mode. Writes are
version-checked against document hash (already computed by
`mytex-vault`). Conflicts surface as last-write-wins with a UI prompt.
Multi-device offline editing is a v3 concern.

**D13. Phase 2b is split into five sub-milestones.**
`mytex-server` + `mytex-crypto` + `mytex-sync` + `apps/web` is too
much to land atomically. Order:

- **2b.1** — Server skeleton + user auth. Axum, Postgres, sessions.
  Plaintext blob storage at rest. No vault endpoints.
- **2b.2** — Server vault + index + token endpoints; `mytex-sync`
  client. Desktop gains a remote workspace. Still plaintext.
- **2b.3** — `mytex-crypto` + session-bound decryption. Retrofit
  encryption onto 2b.2's endpoints.
- **2b.4** — `context.propose` + MCP HTTP/SSE transport. OAuth 2.1
  + PKCE for agent tokens lands here (not 2b.1, because users don't
  need it until agents hit HTTP MCP).
- **2b.5** — `apps/web` web client + WASM crypto.

**D14. No managed backend (no Supabase).**
Supabase buys us ~2–3 weeks on auth flows but costs us a 6–7
container self-host stack, a heavyweight external dependency in
fast flux, and a fork between SaaS and self-host paths. The
interesting parts of Mytex (MCP protocol, session-bound decryption,
audit chain) are custom and Supabase does not help with any of them.
Self-host stays at 2 containers (server + Postgres) which is the
story we want to tell. Stack tight: `sqlx` + `argon2` + `axum` SSE +
`argon2` migrations. Industry-light, not industry-thin.

**D15. Session model — opaque server-side sessions, not JWT.**
One-click revoke is a product feature (ARCH §5.2); JWTs would need
a denylist to support that, which defeats statelessness. `mytex-auth`
already uses opaque tokens for MCP agents; user-login sessions use
the same shape (opaque `mtx_*` prefix, Argon2id-hashed at rest,
revocable). Per-request DB work is already paid by audit logging.
Single-service, single-Postgres shape has no distributed-validation
surface for JWT to win on. Federated SSO later (Google/GitHub) fits
this cleanly — we receive a JWT from the IdP and issue our own
opaque session.

**D16. Auth implementation — rolled, not a library.**
OAuth 2.1 + PKCE is ~500 lines of well-specified Rust. Pulling in
`oxide-auth` or similar adds config complexity we don't need and
couples the auth surface to a dep's opinions. `argon2` is already
a workspace dep (used in `mytex-auth`); reuse it. `sqlx` offline-
checked queries match `rusqlite` ergonomics elsewhere in the repo.

**D17. Crate layout — `crates/mytex-server`, lib + bin.**
Same shape as `mytex-mcp`: a library crate exposing a library for
tests and integration, plus a `mytex-server` binary for the docker
image. Keeps `apps/` reserved for end-user clients (desktop, later
web); servers live under `crates/`.

### Phases

#### Phase 2a — Multi-vault desktop + workspace switcher

No network yet. Teach the desktop that "vault" is plural.

- Desktop state becomes `Workspaces { active: Id, vaults: Map<Id, OpenVault> }`.
- Workspace registry at `~/.mytex/workspaces.json` (distinct from the
  per-vault `.mytex/` inside each root).
- Switcher UI (sidebar dropdown + keyboard command).
- Frontend routes become `/w/:workspace/documents`, etc.
- Per-workspace audit logs, tokens, indices — already isolated by
  path, needs a registry that enumerates them.
- Remote workspaces (Phase 2b) slot into the same switcher later.
- **Crates touched:** `mytex-desktop` only.
- **Unblocks:** use case 5 locally.
- **Cuts:** no cross-workspace search; no "all workspaces" view.

#### Phase 2b — Server + remote driver + sync (five sub-milestones, D13)

Too big to land atomically. Each sub-milestone is independently
useful and testable.

##### 2b.1 — Server skeleton + user auth (plaintext)

Gets the deployment shape real before anything depends on it.

- **New crate:** `crates/mytex-server` (axum, lib + bin, D17).
- **Postgres schema** via `sqlx` migrations: `accounts`, `sessions`,
  `memberships` (latter unused until 2c; schema exists so we don't
  migrate later). `tenant_id` columns present and NOT NULL even
  though a single implicit tenant is enforced in 2b.1.
- **Auth flow** — email + password signup, login, logout, `me`.
  Password hash via `argon2` (reuse workspace dep). Session token
  is opaque `mtx_*` + Argon2id hash at rest (D15, mirrors
  `mytex-auth`). `last_used` updated on every authenticated request;
  revoke = delete row.
- **Endpoints:** `/healthz`, `POST /v1/auth/signup`,
  `POST /v1/auth/login`, `DELETE /v1/auth/logout`,
  `GET /v1/auth/me`, `GET /v1/auth/sessions` (current user's
  sessions with `last_used`).
- **Session middleware** parses `Authorization: Bearer <token>`,
  looks up + validates, attaches account to request extensions.
  60-second in-memory cache in front of the DB lookup — bounded
  staleness for revocation (acceptable), cheap for bursty clients.
- **Packaging:** multi-stage `Dockerfile` + `docker-compose.yml`
  (server + Postgres, optional Caddy for TLS). Dev uses plain HTTP
  on `localhost:8080`.
- **Tests:** `sqlx::test` macro for integration tests against a
  real Postgres (skip when `TEST_DATABASE_URL` unset). Covers
  signup → login → middleware accept/reject → logout.
- **No OAuth 2.1 authorization-code flow yet.** That's for agent
  tokens, lands in 2b.4 when MCP HTTP does. Users log in with
  password; the token returned is an opaque session, same shape
  `mytex-auth` already uses.
- **No vault endpoints yet** (2b.2).
- **Cuts:** no email verification, no password reset, no rate
  limiting beyond what axum/tower gives; all additive in 2b.x.

##### 2b.2 — Vault + index endpoints, `mytex-sync` client

Server speaks the `VaultDriver` + `Index` + token + audit surface
over HTTP. Desktop workspace `kind = "remote"` becomes usable.
Still plaintext at rest; encryption retrofits in 2b.3.

**New crate: `crates/mytex-sync`**

Client-side library that turns a running `mytex-server` into a valid
implementation of the existing traits, so desktop code that calls
`VaultDriver::list` works unchanged against a remote workspace.

Target shape:

```rust
pub struct RemoteConfig {
    pub server_url: Url,
    pub tenant_id: Uuid,        // resolved at login; one per account today
    pub session_token: String,  // raw "mtx_*" bearer
}

pub struct RemoteVaultDriver { /* reqwest::Client + RemoteConfig */ }
#[async_trait] impl VaultDriver for RemoteVaultDriver { /* list/read/write/delete */ }

pub struct RemoteIndex { /* shared client */ }
// Mirrors the subset of `mytex-index::Index` that the MCP/desktop surface
// actually calls: search(SearchQuery), list(ListFilter), all_edges(),
// outbound_links(id), backlinks(id).
```

Local SQLite index as a best-effort read cache — populated on every
successful list/search; served optimistically on cache hit with a
5 s TTL, falls through to the server on miss. A `ServerChanges` SSE
subscription (server side) invalidates cache entries per document
id on remote writes.

**Server-side schema additions (new migration):**

```sql
CREATE TABLE documents (
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    doc_id      TEXT NOT NULL,
    type_       TEXT NOT NULL,
    visibility  TEXT NOT NULL,
    frontmatter JSONB NOT NULL,   -- serialized Frontmatter
    body        TEXT NOT NULL,    -- markdown, plaintext in 2b.2
    version     TEXT NOT NULL,    -- sha256:... of canonical serialization
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, doc_id)
);
CREATE INDEX documents_tenant_type_idx ON documents (tenant_id, type_);
CREATE INDEX documents_tenant_visibility_idx ON documents (tenant_id, visibility);

-- tsvector column + GIN index for full-text search, matches the FTS5
-- shape mytex-index offers locally.
ALTER TABLE documents ADD COLUMN tsv tsvector
  GENERATED ALWAYS AS (to_tsvector('english', body)) STORED;
CREATE INDEX documents_tsv_idx ON documents USING GIN (tsv);

CREATE TABLE doc_tags (
    tenant_id UUID NOT NULL,
    doc_id    TEXT NOT NULL,
    tag       TEXT NOT NULL,
    PRIMARY KEY (tenant_id, doc_id, tag),
    FOREIGN KEY (tenant_id, doc_id) REFERENCES documents(tenant_id, doc_id) ON DELETE CASCADE
);

CREATE TABLE doc_links (
    tenant_id UUID NOT NULL,
    source    TEXT NOT NULL,
    target    TEXT NOT NULL,
    PRIMARY KEY (tenant_id, source, target),
    FOREIGN KEY (tenant_id, source) REFERENCES documents(tenant_id, doc_id) ON DELETE CASCADE
);

-- Per-tenant audit log (matches mytex-audit's JSONL shape, but in Postgres).
CREATE TABLE audit_entries (
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    seq         BIGINT NOT NULL,
    ts          TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor       TEXT NOT NULL,       -- "owner" | "tok:<id>" | "account:<uuid>"
    action      TEXT NOT NULL,
    document_id TEXT,
    scope_used  TEXT[] NOT NULL DEFAULT '{}',
    outcome     TEXT NOT NULL CHECK (outcome IN ('ok','denied','error')),
    prev_hash   TEXT NOT NULL,       -- hex
    hash        TEXT NOT NULL,       -- hex, computed over canonical JSON
    PRIMARY KEY (tenant_id, seq)
);
```

**Endpoint surface** (all under `/v1`, all behind session middleware):

| Method | Path                                  | Notes                         |
| ---    | ---                                   | ---                           |
| GET    | `/tenants`                            | list memberships for caller   |
| GET    | `/t/{tenant_id}/vault/docs`           | list documents (filters via query params) |
| GET    | `/t/{tenant_id}/vault/docs/{doc_id}`  | read one                      |
| PUT    | `/t/{tenant_id}/vault/docs/{doc_id}`  | write one; returns new version |
| DELETE | `/t/{tenant_id}/vault/docs/{doc_id}`  | remove                        |
| GET    | `/t/{tenant_id}/index/search`         | FTS + filters                 |
| GET    | `/t/{tenant_id}/index/list`           | index-only list, no bodies    |
| GET    | `/t/{tenant_id}/index/graph`          | `{nodes, edges}`              |
| GET    | `/t/{tenant_id}/index/backlinks/{id}` | backlinks for a doc           |
| GET    | `/t/{tenant_id}/tokens`               | list MCP tokens for tenant    |
| POST   | `/t/{tenant_id}/tokens`               | issue MCP token               |
| DELETE | `/t/{tenant_id}/tokens/{id}`          | revoke                        |
| GET    | `/t/{tenant_id}/audit`                | paginated audit entries       |
| GET    | `/t/{tenant_id}/events`               | SSE; fires on `vault://changed` |

Error shape reuses `crates/mytex-server/src/error.rs::ApiError` with
the tag set aligned to `MCP.md` §7 (`unauthorized` covers out-of-scope
+ missing + revoked; `version_conflict` for write on stale base; etc.).

**Desktop changes:**

- `WorkspaceEntry` grows optional fields: `server_url`, `session_token`
  (OS keychain; start with plaintext in `.mytex/` and migrate to
  `keyring` crate in 2b.3 with the unlock flow).
- New first-run action: **"Connect to a server"** alongside
  **"Choose vault folder"**. Collects server URL + email + password,
  calls `/v1/auth/login`, stores the resulting session, picks the
  first available tenant (prompts if multiple later).
- `state::open_workspace` routes on `entry.kind`:
  - `"local"` → today's `PlainFileDriver` + local `Index`.
  - `"remote"` → `RemoteVaultDriver` + `RemoteIndex` from `mytex-sync`.
- Everything downstream (commands, views) is trait-driven and keeps
  working. `WorkspaceSwitcher` shows a small "☁" badge for remote.

**Cuts from 2b.2:**

- No encryption at rest (→ 2b.3).
- No OAuth 2.1 PKCE for agent tokens (→ 2b.4). MCP tokens issued via
  POST `/v1/t/{tid}/tokens` remain session-backed for now (authed via
  bearer, not via OAuth).
- No offline write queue. If the network drops mid-edit we surface an
  error and preserve the unsaved body in the UI; no automatic retry.
- No cross-tenant search. Scope stays `/t/{tenant_id}/...`.

**Open questions for 2b.2:**

- **Where does the MCP server run for a remote workspace?** Desktop
  keeps running the local `mytex-mcp` stdio server against the
  `RemoteVaultDriver`/`RemoteIndex`, so agents talking to the desktop
  continue to work transparently. Agents talking directly to the
  server use 2b.4's HTTP/SSE transport. Likely both ship.
- **Cache invalidation on SSE vs polling** — start with SSE; fall
  back to polling if SSE proves unreliable on hosted infra.
- **Audit chain across local + server.** If the same tenant is
  written from two clients, the per-tenant audit chain on the
  server is authoritative. Desktop's local audit JSONL becomes a
  cache/mirror for local workspaces only.

##### 2b.3 — `mytex-crypto` + session-bound decryption

Encryption at rest + the key hierarchy from ARCH §3.4 / Q3.

- **New crate:** `crates/mytex-crypto` — libsodium or age wrappers.
  Key hierarchy: `passphrase → master key (device-only) → session
  key (TTL-bound) → per-doc keys`. WASM-compilable for the web
  client (2b.5).
- **Server changes:** store blobs encrypted; server holds a
  short-lived session key published by an active client. When no
  session key is live → falls back to opaque blobs, reads return
  `-32006 / vault_locked`. Strict-E2EE opt-out is a per-account flag.
- **Desktop changes:** unlock flow — passphrase once per session,
  cached in OS keychain (keyring crate); session-key heartbeat to
  server while unlocked.

##### 2b.4 — `context.propose` + MCP HTTP/SSE

Finally makes the server reachable by external agents over MCP.

- **MCP transport** on `mytex-server`: JSON-RPC over HTTP + SSE
  per `MCP.md` §2.2. Same tools, same error model.
- **OAuth 2.1 + PKCE** for agent token issuance — rolled (D16).
  Authorization-code + PKCE, audience-bound bearer tokens, issued
  by the logged-in user via the desktop/web UI. Opaque token shape
  (still D15) — OAuth defines the *issuance* flow, not the token
  encoding.
- **`context.propose`** lands on both surfaces. Desktop + web
  proposal review queue for admins (2c-ready).

##### 2b.5 — Web client

Separate app, feature-parity with desktop's remote-workspace mode.

- **New app:** `apps/web` — Vite + React + Tailwind, mirrors
  `apps/desktop/src/` components as much as possible.
- **WASM crypto:** `mytex-crypto` compiled to wasm32 target for
  in-browser decryption. Session-key publish happens from the
  browser once the user unlocks.
- **No stdio MCP** (browser can't spawn processes) — hosted
  integrations go through the server's HTTP/SSE MCP from 2b.4.

**Unblocks after full 2b:** use case 2 end-to-end. Also a power-
user flavor of use case 1 (own server, own devices).

#### Phase 2c — Teams and org context

Multi-tenant on the same server, plus team semantics.

- **Crates touched:**
  - `mytex-server` — membership table, role middleware, workspace
    routing (`/w/:id/...`), invite flow, first-user-is-admin (D10).
  - `mytex-vault` — seed `org/` type directory, `org:` visibility
    label.
  - `mytex-auth` — workspace-aware tokens, role-derived default
    scopes (D11).
  - `mytex-desktop` + `apps/web` — team management UI (members,
    invites, role change), org-context editor (admin), propose-to-org
    (member).
- **SaaS is just multi-tenant signups on the same image.**
  On-prem is the same image inside the customer's firewall.
- **Unblocks:** use cases 3 and 4.
- **Cuts:** no billing integration (out of scope per user);
  no SCIM/SAML (add if enterprise customers ask); no per-doc ACLs.

### New crates / apps summary

| Name            | Kind  | Phase   | Role                                          |
| ---             | ---   | ---     | ---                                           |
| `mytex-server`  | crate | 2b.1+   | Axum HTTP + Postgres; Docker-packaged         |
| `mytex-sync`    | crate | 2b.2    | Client-side `VaultDriver` over HTTPS          |
| `mytex-crypto`  | crate | 2b.3    | Session-bound key hierarchy; WASM-compilable  |
| `apps/web`      | app   | 2b.5    | React web client (parallel to desktop)        |

Phases 2a and 2c add no new crates — both extend existing ones.

### Scope cuts (explicit)

- No CRDTs (D12).
- No mobile app, no browser extension.
- No federation between self-hosted servers.
- No per-document ACLs — reuse `visibility` + roles.
- No billing in 2c (deferred).
- No offline-first multi-device editing — online-first, version-checked.
- No cross-workspace search in 2a.
- No SSO/SCIM/SAML in 2c initial cut.

### Open questions

Resolved this session (captured in decisions above):

- ✅ **DB toolchain.** `sqlx` with runtime-checked queries in 2b.1;
  migrate to `query!` + offline cache when CI has Postgres.
- ✅ **Web client tech.** Vite + React + Tailwind, mirror desktop.
- ✅ **MCP transport for team workspaces.** Both — desktop runs local
  `mytex-mcp` against `RemoteVaultDriver` (2b.2); hosted integrations
  hit the server's HTTP/SSE MCP (2b.4).
- ✅ **Managed backend?** No Supabase (D14). Self-host stays at 2
  containers.
- ✅ **Session model.** Opaque server-side, not JWT (D15).

Still open:

- **OAuth provider for SaaS.** We run our own IdP (rolled, D16) for
  Phase 2. Integrating federated SSO (Google, GitHub, Okta, WorkOS)
  and making it pluggable for self-host enterprise customers is a
  Phase 2c+ question.
- **Conflict UI (2b.2+).** Version-miss on write returns
  `version_conflict`. UX options: inline diff + pick-a-side, or
  re-open the editor with both versions for a manual merge. TBD when
  2b.2 lands and we can exercise the case.
- **Invite UX (2c).** Email magic link, shareable join-code link, or
  both. Email needs an SMTP dep and deliverability story; join link
  is simpler but more copy-paste.
- **Keychain story for session tokens (2b.2/2b.3).** Today the desktop
  stores the Anthropic API key in plaintext in `.mytex/settings.json`.
  Remote session tokens will want OS keychain (`keyring` crate) before
  we ship a distribution build — track as a Phase 2b.3 dep since the
  unlock flow lands with crypto.
- **Sync cache TTL + SSE fallback (2b.2).** Start at 5 s TTL + SSE
  invalidation; revisit if SSE proves flaky on hosted infra.

---

## Out of scope / deferred

- Cloud sync + session-bound decryption — tracked in Phase 2 above.
- `context.propose` write-back flow — tracked in Phase 2 above.
- HTTP API — tracked in Phase 2 above.

---

## Repo layout

```
mytex/
├─ Cargo.toml                 workspace root, Apache-2.0, MSRV 1.75
├─ crates/
│  ├─ mytex-vault/            ✅ shipped
│  ├─ mytex-audit/            ✅ shipped
│  ├─ mytex-auth/             ✅ shipped
│  ├─ mytex-index/            ✅ shipped
│  ├─ mytex-mcp/              ✅ shipped
│  └─ mytex-server/           ✅ Phase 2b.1
│     ├─ src/                 lib + bin (axum HTTP API)
│     ├─ migrations/          sqlx migrations (Postgres)
│     ├─ tests/               integration tests (need live Postgres)
│     ├─ Dockerfile           multi-stage, debian-slim runtime
│     ├─ docker-compose.yml   postgres + server; dev profile
│     └─ .env.example         reference env vars for compose
├─ apps/
│  └─ desktop/                ✅ Phase 2a
│     ├─ src-tauri/           Rust (mytex-desktop crate)
│     └─ src/                 React + Vite + TS + Tailwind
└─ docs/
   ├─ ARCHITECTURE.md         v1 contract + Phase 2 overview
   ├─ FORMAT.md               vault format spec + Phase 2 planned additions
   ├─ MCP.md                  MCP surface spec + Phase 2 roadmap
   ├─ reconciled-v1-plan.md   v1 decisions (D1–D6)
   ├─ comparison-architecture.md  alternate proposal (input only; superseded)
   └─ implementation-status.md   this file (Phase 2 decisions D7–D17 + progress)
```

---

## Development quick-reference

### Running the full test suite

```bash
# Without Postgres: 109 tests pass (mytex-server integration tests skip).
cargo test --workspace

# With Postgres: 118 tests pass. Spin up a throwaway container:
docker run --rm -d --name mytex-test-pg \
  -e POSTGRES_USER=mytex -e POSTGRES_PASSWORD=testpw -e POSTGRES_DB=mytex_test \
  -p 5555:5432 postgres:16-alpine

DATABASE_URL="postgres://mytex:testpw@localhost:5555/mytex_test" \
  cargo test --workspace

docker stop mytex-test-pg
```

`sqlx::test` creates a fresh database per test function, so there is
no state bleed between tests. The throwaway container is for dev
ergonomics only; CI will want a persistent Postgres service.

### Running mytex-server locally

```bash
# From crates/mytex-server/:
cp .env.example .env
docker compose up            # postgres + server on localhost:8080
curl http://localhost:8080/healthz

# Or for a hot-reload dev loop on the server:
docker compose up -d postgres
DATABASE_URL="postgres://mytex:mytex-dev-password@localhost/mytex" \
  cargo run -p mytex-server
```

### Running the desktop app

```bash
cd apps/desktop
npm install
npm run tauri dev
```

First run shows the vault picker; registers the chosen directory as
a workspace in `~/.mytex/workspaces.json`. Subsequent launches
auto-open the active workspace.
```
