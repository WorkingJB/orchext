# Ourtex — Implementation Status

Master index. Running status of the v1 build, updated after each
crate or significant milestone. Other docs describe *intent*
(`ARCHITECTURE.md`, `FORMAT.md`, `MCP.md`, `reconciled-v1-plan.md`);
this one describes *state*. Per-phase detail lives in
[`phases/`](phases/) so any one doc stays readable; a new session
should be able to open this file and the phase it's working in, and
know exactly where things stand.

**Per-phase docs:** keep each under ~500 lines. If a phase is pushing
that limit, consolidate scope or break out a sub-phase.

---

## Snapshot

**Last updated:** 2026-04-25

**Toolchain:** Rust 1.95.0 stable (rustup) + Node 20+ for the web /
desktop frontends. wasm-pack 0.14 drives the browser crypto build.
Workspace at repo root.

**Test totals:** 153/153 passing with `DATABASE_URL` set; 133/133
without the DB-required suite (Rust only — `apps/web` has no JS test
suite yet). +5 unit tests this round, all in the new `cookies`
module.

**Scope shuffle 2026-04-25:** four scope changes folded in one pass.
(1) **Graph view dropped.** Desktop's `GraphView.tsx` +
`react-force-graph-2d` removed; web never adopted it. The view didn't
earn its UI weight against the documents list. (2) **2b.4 closed**
without onboarding parity — desktop's Anthropic-keyed onboarding chat
needs a server-mediated route the browser can call, deferred to
Phase 3 platform. (3) **2b.5 narrowed and started.** Begins with web
auth hardening (httpOnly session cookie + double-submit CSRF),
followed by OAuth 2.1 + PKCE for agent tokens, then MCP HTTP/SSE,
then `context.propose`. (4) **Phase 2c absorbed into Phase 3 platform**
alongside web onboarding chat and OS keychain — see
[`phases/phase-3-platform.md`](phases/phase-3-platform.md). Phase 3a
rebrand still kicks off the post-platform work.

**Rebrand 2026-04-21:** product renamed `mytex` → `ourtex`. All
crates, bundle identifiers, env vars (`MYTEX_*` → `OURTEX_*`), vault
directory (`.mytex` → `.ourtex`), and token prefix (`mtx_` → `otx_`)
renamed in place. No backwards-compat shims — existing installs and
databases must be rebuilt.

**Rebrand planned 2026-04-22 (executes in Phase 3a):** product will
rename `ourtex` → `orchext` (orchestration + context) once Phase
2b.4 wraps. Same playbook: `OURTEX_*` → `ORCHEXT_*`, `.ourtex` →
`.orchext`, `otx_*` → `ocx_*`, GitHub org/repo rename. Executes as
the kickoff of Phase 3a alongside the `type: task` / `type: skill`
seed types, because Phase 3 also absorbs the scope expansion into
task aggregation + agent orchestration. Plan detail in
[`phases/phase-3a-rebrand-tasks.md`](phases/phase-3a-rebrand-tasks.md).

| Crate          | Status        | Unit | Integration | Notes                                  |
|----------------|---------------|-----:|------------:|----------------------------------------|
| `ourtex-vault`  | ✅ shipped     | 12   | 6           | Format parser + `PlainFileDriver`      |
| `ourtex-audit`  | ✅ shipped     | 2    | 5           | Hash-chained JSONL log                 |
| `ourtex-auth`   | ✅ shipped     | 11   | 9           | Opaque tokens + Argon2id + scopes      |
| `ourtex-index`  | ✅ shipped     | 4    | 6           | SQLite + FTS5; search / graph / filter |
| `ourtex-mcp`    | ✅ shipped     | 11   | 22          | JSON-RPC + stdio; rate limit + fs watcher |
| `ourtex-desktop`| ✅ 2a + 2b.2 + 2b.3 | 7 | —           | Multi-vault + remote connect + unlock/lock |
| `ourtex-server` | ✅ Phase 2b.3 | 20   | 20          | Auth + vault + index + tokens + audit + crypto |
| `ourtex-sync`   | ✅ 2b.2 + 2b.3 | 0   | —           | `RemoteVaultDriver` + crypto control calls |
| `ourtex-crypto` | ✅ 2b.3 + wasm32 | 13 | —           | Argon2id KDF + XChaCha20-Poly1305 AEAD; browser build clean |
| `ourtex-crypto-wasm` | ✅ 2b.4 | —  | —               | wasm-bindgen surface; 4 ops: generateSalt/ContentKey, wrap/unwrap |
| `ourtex-web`    | ✅ 2b.4 + 🚧 2b.5 | — | —           | Login + tenant picker + unlock + doc CRUD + tokens + audit; cookie/CSRF auth in flight |

**In flight:** Phase 2b.5 — auth hardening + agent surface. Opened
2026-04-25 with the web auth migration: server emits an httpOnly
`ourtex_session` cookie alongside a readable `ourtex_csrf` cookie on
login/signup, and accepts either bearer (desktop) or cookie (web) on
authenticated routes. State-changing cookie-authed requests must
double-submit CSRF via `X-Ourtex-CSRF` header. Web client drops its
`localStorage` token entirely and probes `/v1/auth/me` on load to
classify session state. Subsequent 2b.5 slices: OAuth 2.1 + PKCE for
agent tokens, MCP HTTP/SSE transport, `context.propose`. Details in
[`phases/phase-2b5-auth-mcp.md`](phases/phase-2b5-auth-mcp.md) (TBD);
forward plan in [`phases/phase-2-plan.md`](phases/phase-2-plan.md).

**Just shipped:** Phase 2b.4 closed 2026-04-25. Web client gained
login + signup, tenant picker, browser unlock with WASM-side
KDF/AEAD, 4-minute content-key heartbeat, doc CRUD with
base-version optimistic concurrency, tokens admin, and audit list.
Graph view dropped from both clients; onboarding chat moved to
Phase 3 platform.

---

## Phase docs

### Shipped (frozen)

- [`phases/phase-1-core.md`](phases/phase-1-core.md) — Core v1:
  vault, audit, auth, index, mcp, desktop (incl. Phase 2a
  multi-vault).
- [`phases/phase-2b1-server.md`](phases/phase-2b1-server.md) —
  Server skeleton + auth (axum, Postgres, sessions).
- [`phases/phase-2b2-remote-vault.md`](phases/phase-2b2-remote-vault.md) —
  Tenant-scoped vault/index/token/audit HTTP endpoints + `ourtex-sync`
  client + desktop remote workspaces.
- [`phases/phase-2b3-encryption.md`](phases/phase-2b3-encryption.md) —
  `ourtex-crypto` + session-bound decryption; encrypted
  `body_ciphertext`; desktop unlock/lock + heartbeat.
- [`phases/phase-2b4-web.md`](phases/phase-2b4-web.md) — `apps/web` +
  `ourtex-crypto-wasm`; login, tenant picker, unlock, doc CRUD,
  tokens, audit. Closed 2026-04-25 without graph or onboarding chat.

### In flight

- [`phases/phase-2-plan.md`](phases/phase-2-plan.md) (Phase 2b.5) —
  Auth hardening (cookie + CSRF) opened 2026-04-25, then OAuth 2.1
  PKCE, MCP HTTP/SSE, `context.propose`.

### Planned

- [`phases/phase-2-plan.md`](phases/phase-2-plan.md) — Phase 2 goals,
  decisions D7–D17, remaining 2b.5 slices, scope cuts, open
  questions.
- [`phases/phase-3-platform.md`](phases/phase-3-platform.md) —
  Teams + invites (formerly Phase 2c), web onboarding chat, OS
  keychain. Bundles the work pushed out of 2b.4 and 2b.5 narrowing.
- [`phases/phase-3a-rebrand-tasks.md`](phases/phase-3a-rebrand-tasks.md) —
  Rebrand `ourtex` → `orchext` + vault-native `type: task` and
  `type: skill` seed types (FORMAT v0.2). Kicks off after Phase 3
  platform wraps.
- [`phases/phase-3b-integrations.md`](phases/phase-3b-integrations.md) —
  First external task integration (Todoist) + visibility-driven
  storage tier (`task_projection`) + server-held integration
  credentials. Introduces decisions D18, D22–D26.
- [`phases/phase-3c-task-expansion.md`](phases/phase-3c-task-expansion.md) —
  Linear / Jira / Asana / MS To Do adapters + team-inbox aggregation
  (depends on the team workspaces shipped in Phase 3 platform).
  Decisions D27–D30.
- [`phases/phase-3d-agent-observer.md`](phases/phase-3d-agent-observer.md) —
  Agent sessions observer-only: `orchext-agents` crate, heartbeat
  protocol, client-encrypted transcripts, activity panes. Decisions
  D31–D35.
- [`phases/phase-3e-orchestration.md`](phases/phase-3e-orchestration.md) —
  Full orchestration surface: atomic task checkout, HITL approval
  gates, runtime skill injection, shared team agents, goal
  ancestry. Decisions D36–D42.
- [`phases/phase-4-installers.md`](phases/phase-4-installers.md) —
  Desktop distribution & installers (signed macOS DMG, Windows MSI,
  Linux, auto-updater). Renumbered from Phase 3 on 2026-04-22.

---

## Out of scope / deferred

- Cloud sync + session-bound decryption — shipped, see
  [`phases/phase-2b3-encryption.md`](phases/phase-2b3-encryption.md).
- `context.propose` write-back flow — planned for Phase 2b.5.
- HTTP API — shipped, see
  [`phases/phase-2b2-remote-vault.md`](phases/phase-2b2-remote-vault.md).
- Desktop installers / signed builds — planned for Phase 4
  (formerly Phase 3; renumbered 2026-04-22).

---

## Repo layout

```
ourtex/
├─ Cargo.toml                 workspace root, Apache-2.0, MSRV 1.75
├─ crates/
│  ├─ ourtex-vault/            ✅ shipped
│  ├─ ourtex-audit/            ✅ shipped
│  ├─ ourtex-auth/             ✅ shipped
│  ├─ ourtex-index/            ✅ shipped
│  ├─ ourtex-mcp/              ✅ shipped
│  ├─ ourtex-server/           ✅ Phase 2b.3
│  │  ├─ src/                 lib + bin (axum HTTP API)
│  │  ├─ migrations/          sqlx migrations (Postgres)
│  │  ├─ tests/               auth_flow.rs + vault_flow.rs + crypto_flow.rs (need live Postgres)
│  │  ├─ Dockerfile           multi-stage, debian-slim runtime
│  │  ├─ docker-compose.yml   postgres + server; dev profile
│  │  └─ .env.example         reference env vars for compose
│  ├─ ourtex-sync/             ✅ 2b.2 + 2b.3 — RemoteVaultDriver + crypto control
│  ├─ ourtex-crypto/           ✅ 2b.3 + wasm32 — Argon2id KDF + XChaCha20-Poly1305 AEAD
│  └─ ourtex-crypto-wasm/      ✅ 2b.4 — wasm-bindgen surface for the browser
├─ apps/
│  ├─ desktop/                ✅ Phase 2a
│  │  ├─ src-tauri/           Rust (ourtex-desktop crate)
│  │  └─ src/                 React + Vite + TS + Tailwind
│  └─ web/                    🚧 Phase 2b.4 — in flight
│     ├─ src/                 React + Vite + TS + Tailwind (no Tauri)
│     └─ src/wasm/            wasm-pack output (generated, gitignored)
└─ docs/
   ├─ ARCHITECTURE.md         v1 contract + Phase 2 overview
   ├─ FORMAT.md               vault format spec + Phase 2 planned additions
   ├─ MCP.md                  MCP surface spec + Phase 2 roadmap
   ├─ reconciled-v1-plan.md   v1 decisions (D1–D6)
   ├─ comparison-architecture.md  alternate proposal (input only; superseded)
   ├─ implementation-status.md   this file — master index
   └─ phases/                 per-phase status docs (shipped + planned)
      ├─ phase-1-core.md
      ├─ phase-2b1-server.md
      ├─ phase-2b2-remote-vault.md
      ├─ phase-2b3-encryption.md
      ├─ phase-2b4-web.md
      ├─ phase-2-plan.md
      ├─ phase-3a-rebrand-tasks.md
      ├─ phase-3b-integrations.md
      ├─ phase-3c-task-expansion.md
      ├─ phase-3d-agent-observer.md
      ├─ phase-3e-orchestration.md
      └─ phase-4-installers.md
```

---

## Development quick-reference

### Running the full test suite

```bash
# Without Postgres: 109 tests pass (ourtex-server integration tests skip).
cargo test --workspace

# With Postgres: 118 tests pass. Spin up a throwaway container:
docker run --rm -d --name ourtex-test-pg \
  -e POSTGRES_USER=ourtex -e POSTGRES_PASSWORD=testpw -e POSTGRES_DB=ourtex_test \
  -p 5555:5432 postgres:16-alpine

DATABASE_URL="postgres://ourtex:testpw@localhost:5555/ourtex_test" \
  cargo test --workspace

docker stop ourtex-test-pg
```

`sqlx::test` creates a fresh database per test function, so there is
no state bleed between tests. The throwaway container is for dev
ergonomics only; CI will want a persistent Postgres service.

### Running ourtex-server locally

```bash
# From crates/ourtex-server/:
cp .env.example .env
docker compose up            # postgres + server on localhost:8080
curl http://localhost:8080/healthz

# Or for a hot-reload dev loop on the server:
docker compose up -d postgres
DATABASE_URL="postgres://ourtex:ourtex-dev-password@localhost/ourtex" \
  cargo run -p ourtex-server
```

### Running the desktop app

```bash
cd apps/desktop
npm install
npm run tauri dev
```

First run shows the vault picker; registers the chosen directory as
a workspace in `~/.ourtex/workspaces.json`. Subsequent launches
auto-open the active workspace.

### Running the web app

```bash
# Requires wasm-pack on PATH (cargo install wasm-pack).
cd apps/web
npm install
npm run dev                  # http://localhost:1430
```

`predev` and `prebuild` hooks run `wasm-pack build` against
`ourtex-crypto-wasm` so the WASM module is always fresh. Set
`OURTEX_SERVER_URL` to override the proxy target
(default `http://localhost:8080`).
