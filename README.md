# Orchext

> API and AI documentation but for you — and your team.

Orchext is a local-first context store for the AI era. You keep a
directory of plain-markdown files describing who you are, what you're
working on, who you work with, and which tools you use; any AI agent
that speaks MCP can read from that directory with a scoped token, and
nothing writes without your review. An optional encrypted cloud tier
lets context follow you across devices and shares it with teammates
and organizations without handing the provider your keys.

## Why

Every AI product is rebuilding the same lossy profile of you from
scratch. Orchext inverts that: one vault you own, many agents you
grant read access to. Moving models, switching tools, or bringing a
new teammate into a project doesn't mean re-teaching anyone who you
are or what the project is about.

## Common use cases

- **Personal context for AI coding / writing assistants.** Keep your
  preferences, current projects, and tool inventory as markdown in
  `~/Orchext/`. Point Claude Desktop, Cursor, or any MCP-speaking
  client at the local MCP server. The agent reads scoped context on
  demand; you review and accept any proposed writes.
- **Shared team context ("orchext" for organizations).** A tenant
  workspace on the cloud tier holds goals, decisions, and
  conventions the whole team should share. Members connect their
  desktop app or web client with their own credentials; the server
  only ever sees encrypted blobs.
- **Portable identity across AI providers.** Provider-specific
  memory systems lock you in. An Orchext vault is a directory of
  markdown with YAML frontmatter — grep-able, diff-able, git-able,
  and editable in any text editor if you ever walk away.
- **Agent sandboxing.** External agents get read-only MCP access by
  default. Proposed writes land in `.orchext/proposals/` for a human
  to approve, so a misbehaving agent can't quietly rewrite your
  context.

## Repo layout

```
apps/
  desktop/             Tauri app (React UI + Rust core)
  web/                 React web client against orchext-server
crates/
  orchext-vault/        VaultDriver trait + PlainFileDriver
  orchext-index/        SQLite + FTS5 search / graph index
  orchext-auth/         Scoped opaque-token auth (ocx_* secrets)
  orchext-audit/        Hash-chained append-only audit log
  orchext-crypto/       Argon2id KDF + XChaCha20-Poly1305 AEAD
  orchext-crypto-wasm/  wasm-bindgen surface for the browser
  orchext-mcp/          MCP server (JSON-RPC stdio + HTTP/SSE)
  orchext-server/       Cloud control-plane (axum + Postgres)
  orchext-sync/         Remote vault driver + crypto control calls
docs/
  ARCHITECTURE.md      System design and key decisions
  FORMAT.md            Vault file format (markdown + frontmatter)
  MCP.md               MCP surface exposed to external agents
  implementation-status.md   Running build status
  phases/              Per-phase implementation plans
```

## Getting started

Prerequisites: Rust 1.75+, Node 20+, and (for the server crate)
Postgres reachable via `DATABASE_URL`.

```bash
# Build the workspace
cargo check --workspace

# Run the desktop app
cd apps/desktop
npm install
npm run tauri dev

# Run the web app (requires wasm-pack on PATH)
cd apps/web
npm install
npm run dev            # http://localhost:1430

# Run the cloud server locally (requires Postgres)
cd crates/orchext-server
cp .env.example .env   # edit DATABASE_URL + ORCHEXT_BIND
cargo run
```

See [`docs/implementation-status.md`](docs/implementation-status.md)
for what's shipped, what's in flight, and which phase doc to open
next.

## License

Apache-2.0.
