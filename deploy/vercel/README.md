# Vercel deployment

Vercel hosts the static `apps/web` build — the Vite/React SPA. Two
projects, one per environment:

| Project | Domain | Deploys from |
|---|---|---|
| `orchext-web-prod` | `app.orchext.ai` | `main` |
| `orchext-web-test` | `test-app.orchext.ai` | `develop` (or `main` until a `develop` branch exists) |

Vercel itself is configured through its UI/API rather than a
committed config file. There is no `fly.toml`-style equivalent worth
maintaining here. This README captures the *expected* state so it
can be re-created or audited.

## First-time bring-up

For each project (`-prod` and `-test`):

1. **Create the project** in Vercel, point it at this repo.
2. **Build settings**:
   - Framework preset: **Vite**
   - Root directory: `apps/web`
   - Build command: `npm run build`
   - Output directory: `dist`
   - Install command: `npm ci`
3. **Production branch**: `main` for prod project; `develop` for test
   project.
4. **Domains**: bind `app.orchext.ai` (or `test-app.orchext.ai`) and
   add the CNAME record at the registrar pointing to
   `cname.vercel-dns.com`.
5. **Environment variables** (per project, all environments):
   - `VITE_ENV_NAME` = `production` or `test` (cosmetic — drives any
     env-banner UI).
6. **Rewrites**: handled by [`apps/web/vercel.json`](../../apps/web/vercel.json),
   which both projects share. Each rewrite is gated by a `has` clause
   matching on the request `host` header — `app.orchext.ai` rewrites
   to `orchext-prod.fly.dev`, `test-app.orchext.ai` rewrites to
   `orchext-test.fly.dev`. A trailing fallback rule sends any other
   host (preview deploys, `*.vercel.app`) to the test API so previews
   never accidentally hit production data.

   Why one `vercel.json` works for both projects: host-based `has`
   matching makes the destination conditional on the inbound host,
   which is the per-environment signal we need. No env-var
   substitution required, no per-branch divergence.

## Why no Vercel Functions

We deliberately do **not** use Vercel Functions (Edge or Node) to
proxy the API. Reasons:

- The Rust API + Postgres needs a long-running process — Vercel
  Functions don't fit that shape.
- Rewrites are zero-runtime: Vercel just forwards the request without
  running our code. Lower latency, no cold start, no per-invocation
  billing.
- Splitting the API origin onto Fly keeps the deploy story
  symmetric with self-hosters (same `Dockerfile`, just running
  somewhere else).

## What lives in this repo vs. in Vercel

| Lives in repo | Lives in Vercel |
|---|---|
| `apps/web/` source | Project itself, build/install config |
| `apps/web/vercel.json` (host-conditional rewrites) | Domain bindings, DNS verification |
| `VITE_*` env names referenced by code | Env values per environment |
| Build command, root dir, framework preset | Production-branch mapping |

Treat the Vercel UI as a deployment target, not a source of truth.
If a setting is meaningful enough to track, it belongs in code or in
[`docs/DEPLOYMENT.md`](../../docs/DEPLOYMENT.md).
