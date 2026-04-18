# Mytex v1 — Reconciled Plan

This document reconciles `ARCHITECTURE.md` with the alternative proposal
in `comparison-architecture.md`. It picks a side on every format-level
decision so that `FORMAT.md` and the MCP surface can stabilize, and it
restates v1 scope discipline.

Each decision below supersedes both source docs on that specific point.
Everything not listed here stays as it is in `ARCHITECTURE.md`.

---

## Decisions

### D1. Vault layout — **flat types at root**

Keep the current layout:

```
~/Mytex/
├─ .mytex/              config, tokens, audit, index, proposals
├─ identity/
├─ roles/
├─ goals/
├─ relationships/
├─ memories/
├─ tools/
├─ preferences/
├─ domains/
├─ decisions/
└─ attachments/
```

**Not** the `context/`-nested layout from the comparison doc.

Why: Obsidian users can point Obsidian at `~/Mytex/` directly — the
fallback-UI promise is load-bearing. Attachments live at root too (matches
how Obsidian vaults already work). `mytex.yaml` collapses into
`.mytex/config.json`; no separate top-level config file.

Teams path stays open via sibling roots (`~/Mytex/personal/`,
`~/Mytex/acme-team/`), not by adding a `workspaces/` layer inside a
single vault.

### D2. Permission label — **`visibility`**

Frontmatter field is `visibility`, not `sensitivity`.

Values in v1: `personal`, `work`, `public`, plus user-defined labels.

Why: `visibility` matches the user's mental model ("who can see this")
and the UI copy writes itself ("share with agents that can see *work*
context"). `sensitivity` is accurate for the security engine but reads
as jargon to the target user. Internally, the grant engine treats the
label as a scope atom regardless of its name.

### D3. MCP tool names — **`context.*`**

```
context.search(query, scope?)
context.get(id)
context.list(type?, tags?)
context.propose(id, patch)      // v1.1, see D6
```

Not `mytex.search_context` etc. Shorter, reads naturally at the call
site, and the `mytex` prefix is redundant once the server is named.

### D4. Agent auth — **opaque tokens for v1, OAuth-ready for cloud**

v1 (local-only, loopback MCP + local API):

- Opaque per-agent tokens, hashed at rest.
- Scope = set of `visibility` labels.
- Mode = `read` or `read+propose`.
- Expiry, label, last-used, one-click revoke.

Deferred to the cloud-relay milestone:

- OAuth 2.1 with PKCE, audience-bound tokens, protected-resource
  metadata, no token passthrough, HTTPS for non-loopback endpoints.

Why: OAuth is required once remote agents talk to a hosted relay, but
adding it to a loopback-only v1 is cost without benefit. Build the
token service behind an interface so the OAuth path slots in without
touching callers.

### D5. Prompt-injection policy — **provenance yes, sanitization no**

Mytex attaches provenance metadata to every fragment it returns
(`document_id`, `visibility`, `updated_at`, `source`) and marks the
body as untrusted input. It does **not** rewrite, strip, or
re-label instruction-like content inside document bodies.

Why: sanitizing user-authored markdown is fragile and paternalistic —
users write their own context and shouldn't have it silently edited on
the way to the agent. Provenance is cheap and gives the agent what it
needs to defend itself. This is the honest middle between the current
doc ("we don't sanitize") and the comparison doc ("strip and label
instruction-like content").

Retrieval-volume limits from the comparison (max documents / tokens /
snippets per request) are adopted — those are a denial-of-exposure
control, not sanitization.

### D6. Grant model — **current simple model, with retrieval limits added**

Keep the v1 token model (scope, mode, expiry, label). **Add** one
dimension from the comparison doc:

- Retrieval limits: max documents and max tokens per request.

Skip the rest of the comparison doc's 8-dimension grant model for v1
(sensitivity ceiling, network origin, audit level as grant fields,
tag/relationship-neighborhood scopes). Those are fine future work; they
are not worth the UX and engineering cost now.

---

## v1 scope (reaffirmed)

In scope, unchanged from `ARCHITECTURE.md` §8:

1. Tauri desktop app with vault create/browse/edit + graph view.
2. Seed context types per `FORMAT.md`.
3. Local MCP server: `context.search`, `context.get`, `context.list`.
4. Token management UI + audit log viewer.
5. Obsidian vault import.
6. In-app onboarding agent.

**Explicitly rejected additions from the comparison doc:**

- Local HTTP API in v1 — defer to v1.1. One integration surface first.
- `context.propose` write-back flow — defer to v1.1. In-app agent
  covers the onboarding write path; external writes wait.
- Cloud sync prototype in v1 — design-complete only; no code.
- Rich 8-dimension grant model — see D6.

**Adopted from the comparison doc:**

- Retrieval limits on tokens (D6).
- Provenance metadata on returned fragments (D5).
- OAuth 2.1 as the target for cloud-relay auth (D4), deferred.

---

## Open questions still worth the team's time

These are the review questions from the comparison doc that the
decisions above do **not** settle. Each is expanded with the
underlying tension, the options, and a lean.

### Q1. Default visibility labels — should we ship a `private` tier?

**Decided: B.** Ship `public` / `work` / `personal` / `private`.
`private` is a hard floor — never included in a grant unless the grant
explicitly names it, with a distinct UI warning when that happens.
Update `FORMAT.md` accordingly.


**The tension.** The current 3-label scheme (`personal` / `work` /
`public`) conflates two different things: *domain* (which part of my
life this belongs to) and *sensitivity* (how careful I want the system
to be with it). A user's health notes and their favorite pizza place
are both "personal" by domain, but only one should be a surprise to
leak into an agent session.

**Options.**

- **A. Stay with 3 labels.** `personal` / `work` / `public`. Users
  control sensitivity implicitly by choosing which tokens get which
  scopes. Simplest to explain. Fails when a user wants "agents in
  general can see my personal life, but not *this* note."
- **B. Add `private` as a hard floor.** 4 labels: `public` /
  `work` / `personal` / `private`. `private` is never included
  unless a grant explicitly names it, and the UI surfaces that
  clearly ("this agent can read private notes" gets a separate
  warning). Matches Notion's "Private" / iOS's "Hidden" mental model.
- **C. Decouple domain from sensitivity.** Two fields: a domain tag
  (work/personal/…) and a separate sensitivity level (normal/private).
  Maximally flexible, two concepts to teach instead of one.
- **D. Match a password manager.** Everything is private by default,
  agents get explicit per-document share lists. Too heavy for a
  context vault — the whole point is bulk retrieval.

**Lean: B.** The `private` floor is the safety valve users
intuitively expect, and it fits on one label without inventing a
second field. `personal` stays for "about me, not secret"; `private`
means "never ship this by accident."

**To help decide.** Talk to 5–10 target users. Ask: "what's in your
head that you'd *never* want an AI agent to surface without
permission?" If the answers cluster (health, finances, relationships,
therapy notes), a `private` tier earns its keep. If they're scattered
and per-note, option C's per-document sensitivity is more honest.

### Q2. Git integration — invisible, passive, or first-class?

**Decided: B.** Passive history UI — per-document timeline with diff
and restore. No branches, no remotes, no push in the UI. The vault
stays a plain folder, so users are free to run `git init` and push to
their own remote from outside the app; we neither block it nor
advertise it. The product's job is to be simpler than git, not to be
a git client.


**The tension.** A vault-as-folder is git-able for free; the question
is whether the desktop app *knows* about git. Surfacing git in the UI
endorses it as a safe store, which raises our responsibility for
warnings (especially around deleted sensitive content that lingers in
history).

**Options.**

- **A. Invisible.** We don't touch `.git`. Power users run git from
  outside. No history UI. Cheapest, safest.
- **B. Passive history UI.** Per-document timeline with diff and
  "restore previous version." No branches, no remotes, no push. Uses
  git under the hood (or could use a simpler snapshot store). No
  complicated surface area.
- **C. First-class git.** History, revert, plus "connect a remote" for
  self-hosted backup without our cloud tier. Appealing but conflicts
  with D4's cloud-relay model — now there are two sync paths and
  confused users.

**Lean: B.** The main value of git for normal users is *undo*, and
that's worth putting in the UI. Branches and remotes are not. Users
who want push-to-github do it from the CLI and own the
deleted-content leak risk themselves.

**To help decide.** Decide whether "connect your own git remote" is a
marketing point or a support burden. If it's a point of differentiation
against closed memory systems ("your context is in *your* GitHub
repo"), lean toward C. If the user base is non-technical, B is
plenty.

### Q3. Offline-cloud UX — what does the relay return when no device is unlocked?

**The tension.** The cloud tier is end-to-end encrypted. Decryption
keys live on user devices. If a remote agent hits the hosted MCP
relay while every user device is locked or offline, the relay *cannot*
answer with real context — it has only opaque blobs. Something has to
give: either the agent gets a locked state, or we relax E2EE for
some slice of the vault.

**Options.**

- **A. Hard fail.** Relay returns `owner_offline` or similar. Agent
  sees a clear locked state and can tell the user "your Mytex is
  offline, unlock a device to continue." Honest, matches "always
  secure," worst UX.
- **B. User-run keyholder service.** User deploys a lightweight
  decryptor on a VPS or home server (Docker image, one-click
  Fly.io/Railway template). It holds a scoped key and services
  relay requests 24/7. Great for power users, non-starter for mainstream.
- **C. Opt-in hosted unlock for a scope.** User explicitly trusts
  Mytex cloud with, say, their `public` and `work` context. Relay
  can decrypt that slice. `private` and `personal` stay E2EE. Breaks
  the pure E2EE claim for the opted-in slice, but gives honest
  always-on access for what the user chose.
- **D. Short-window cache.** When a device is online, it pushes a
  short-lived decryption token to the relay that expires (say) 1 hour
  after the device goes offline. Limited availability window;
  introduces a new threat model around that token.

**Decided: session-bound cloud decryption (new option E).**

The "have it both ways" framing from the product side is better than
any of options A–D above. Concretely:

- The vault is E2EE at rest on cloud storage. Mytex never holds the
  master key long-term.
- When any user device is online and unlocked, it publishes a
  short-lived **session key** to the cloud, derived from the master
  key and bound to a TTL (default: sliding window, e.g. 24h,
  refreshed automatically while a device is active).
- While a session is live, the cloud relay can decrypt the vault
  on-demand to serve agent integrations server-side. This is the
  default integration path once the user is on the cloud tier —
  fast, always-on, no relay hop back to the desktop.
- When all devices have been offline past the TTL, session keys
  expire and the cloud falls back to opaque blobs. Agents see a
  locked state until a device comes back online.
- Revocation: deleting a device or a manual "lock cloud" action
  expires the session key immediately.

**What this trades.** Strict "cloud can never decrypt your context"
becomes "cloud can decrypt your context only during a session that
one of your devices authorized." That's Bitwarden-class, not
Signal-class, and is the honest claim to make publicly. The master
key still never leaves devices; what goes to the cloud is a
session-scoped derivative.

**What this enables.** Cloud-side semantic search and retrieval,
which would otherwise be impossible under strict E2EE. That's a real
product capability — not just an availability win.

**Implementation notes for the cloud milestone.**

- Key hierarchy: `passphrase → master key (device-only) → session
  key (device + cloud, TTL-bound) → per-document keys (optional)`.
- Session-key rotation on any device-state change (new device,
  removed device, passphrase change).
- Sessions are per-vault, not per-agent; agent tokens still bound by
  `visibility` scope inside the session.
- `private`-tier content should be configurable to require an
  *active* device in the session (not just a cached session key),
  for users who want that extra step. Defaults to the standard
  session behavior.
- Audit log records every cloud-side decryption, same as local
  reads.
- Users who want strict E2EE (no cloud-side decryption ever) can
  opt out; their integrations fall back to the relay-to-device
  model from original option A.

None of this ships in v1 — v1 has no cloud tier — but the decision
unblocks the cloud-milestone design.

### Q4. First MCP client to target — Claude Desktop, Cursor, or both?

**Decided: A primary, B guaranteed secondary.** Claude Desktop is the
polish target — error messages, onboarding flows, and docs are
written against it. Cursor is a required pass in the v1 compatibility
test matrix; anything broken there is a release blocker, but we don't
optimize UX around it.


**The tension.** "Works with MCP" in theory and "works well with
*this specific client*" in practice are different. We need a primary
target to drive ergonomics and error messages. Secondary targets get
a compatibility check, not polish.

**Options.**

- **A. Claude Desktop primary.** Cleanest MCP story, first-party
  spec, target-user overlap (AI-curious generalists). We're building
  with Claude; dogfooding is free.
- **B. Cursor (or Windsurf/Zed) primary.** Developer users are
  self-selecting early adopters: they already manage a vault-shaped
  thing (their repo), they understand tokens, they'll tolerate rough
  edges and give good feedback. Session frequency is higher — the
  editor is open all day.
- **C. Both, equally.** More surface area, slower v1.

**Lean: A primary, B as a guaranteed secondary.** Claude Desktop is
the closest match to the eventual target user and the smoothest
MCP experience today. Commit to Cursor working in the v1 test
matrix because the first wave of users will overlap heavily with
developers, and a broken Cursor experience will dominate early
feedback.

**To help decide.** Which audience do you want on day one? If the
launch narrative is "bring your context to every agent," Claude
Desktop anchors it. If the launch narrative is "stop retyping your
stack to your AI editor," Cursor anchors it. The primary should
match the story you want to tell.

---

## What this changes in the repo

- `ARCHITECTURE.md` §3.5 and §4.1 — no change; decisions D1 and D2 match.
- `ARCHITECTURE.md` §4.5 — no change; D3 matches.
- `ARCHITECTURE.md` §5.2 — add a note that the token service is
  designed to accept an OAuth path (D4).
- `ARCHITECTURE.md` §5.5 — tighten to say Mytex attaches provenance
  metadata but does not sanitize (D5).
- `FORMAT.md` (not yet written) — codify D1, D2, and frontmatter
  fields before any indexer work lands.
- `comparison-architecture.md` — keep as input; this doc supersedes it
  on the points above.
