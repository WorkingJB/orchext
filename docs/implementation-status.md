# Mytex — Implementation Status

Running status of the v1 build. Updated after each crate or significant
milestone. Other docs describe *intent* (`ARCHITECTURE.md`, `FORMAT.md`,
`MCP.md`, `reconciled-v1-plan.md`); this one describes *state*.

A new session should be able to open this file and know exactly where
we are without reading git history.

---

## Snapshot

**Last updated:** 2026-04-18

**Toolchain:** Rust 1.95.0 stable (rustup). Workspace at repo root.

**Test totals:** 55/55 passing.

| Crate         | Status    | Unit | Integration | Notes                                  |
|---------------|-----------|-----:|------------:|----------------------------------------|
| `mytex-vault` | ✅ shipped | 12   | 6           | Format parser + `PlainFileDriver`      |
| `mytex-audit` | ✅ shipped | 2    | 5           | Hash-chained JSONL log                 |
| `mytex-auth`  | ✅ shipped | 11   | 9           | Opaque tokens + Argon2id + scopes      |
| `mytex-index` | ✅ shipped | 4    | 6           | SQLite + FTS5; search / graph / filter |
| `mytex-mcp`   | 🚧 next    | —    | —           | Wire the three services behind JSON-RPC |

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

---

---

## In flight

### `mytex-mcp`

**Scope:** MCP server binary wiring the three services (`vault`, `auth`, `audit`, `index`) behind JSON-RPC over stdio. Implements `initialize`, `tools/list`, `resources/list`, `resources/read`, `resources/subscribe`, and the v1 tools `context.search`, `context.get`, `context.list` per MCP.md §5.

**Planned decisions** (will record actuals once implemented):

- JSON-RPC 2.0 over stdio using line-delimited messages.
- Token passed via `--token` CLI arg, not env (MCP.md §2.1).
- Every call audited via `mytex-audit` with the token's ID as actor.
- Scope evaluation at the service boundary: request comes in → authenticate → narrow scope → pass `allowed_visibility` into index queries.
- Provenance metadata attached to every search/get response (id, visibility, updated, source).
- Retrieval limits enforced server-side before responding.

---

## Out of scope / deferred

- Cloud sync + session-bound decryption (cloud milestone, not v1).
- `context.propose` write-back flow (v1.1).
- HTTP API (v1.1).
- Tauri desktop app (separate milestone).

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
│  └─ mytex-mcp/              🚧 next
└─ docs/
   ├─ ARCHITECTURE.md         v1 contract
   ├─ FORMAT.md               vault format spec
   ├─ MCP.md                  MCP surface spec
   ├─ reconciled-v1-plan.md   decisions doc (supersedes source docs on listed points)
   ├─ comparison-architecture.md  alternate proposal (input only; superseded)
   └─ implementation-status.md   this file
```
