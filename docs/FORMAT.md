# Ourtex Vault Format (v0.1)

The vault format is the long-term contract between a user and Ourtex.
Any version of Ourtex — desktop, cloud, self-hosted — must be able to
read a vault written to this spec. Changes to this document are
versioned.

This spec is deliberately small. If something is not defined here, it
is not part of the format.

---

## 1. Vault layout

A vault is a directory. Its root contains one reserved directory
(`.ourtex/`) and one directory per document `type`.

```
<vault-root>/
├─ .ourtex/             reserved; see §7
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

- The **seed types** above are defined by this spec (§4).
- Users may create additional top-level directories; each becomes a
  **custom type** with no schema hints but full first-class support
  for search, linking, tokens, and sync.
- Nested directories inside a type are allowed and treated as
  organizational sub-groups. They do not affect the type.
- Filenames must match `^[a-z0-9][a-z0-9-]*\.md$`. The filename
  without extension is the document's default `id`.

---

## 2. Document structure

Every document is a UTF-8 markdown file with two parts:

1. A **YAML frontmatter block** delimited by `---` on its own line at
   the top and bottom.
2. A **markdown body** after the closing `---`.

```markdown
---
id: rel-jane-smith
type: relationship
visibility: work
tags: [manager, acme]
links: [[goal-q2-launch]]
created: 2026-04-18
updated: 2026-04-18
---

# Jane Smith

My manager at Acme. Prefers concise written updates over meetings.
Reviews deliverables on Fridays.
```

Frontmatter is mandatory. A file without frontmatter is not a valid
Ourtex document (but may still be stored in the vault and ignored by
the indexer).

---

## 3. Frontmatter fields

### 3.1 Required

| Field        | Type            | Description                                            |
|--------------|-----------------|--------------------------------------------------------|
| `id`         | string          | Stable, unique within the vault. See §3.3.             |
| `type`       | string          | One of the seed types, or a custom type name.          |
| `visibility` | string          | Permission label. See §5.                              |

### 3.2 Optional (reserved)

| Field         | Type                | Description                                         |
|---------------|---------------------|-----------------------------------------------------|
| `tags`        | list of strings     | Free-form labels for filtering and search.          |
| `links`       | list of wikilinks   | Outbound references. See §6.                        |
| `aliases`     | list of strings     | Alternate names this document can be linked by.     |
| `created`     | ISO-8601 date       | Creation date.                                      |
| `updated`     | ISO-8601 date       | Last modification date.                             |
| `source`      | string              | Free-form provenance note (e.g. "onboarding 2026-04-18"). |
| `principal`   | string              | Owner identifier. Always the single user in v1.     |
| `schema`      | string              | Subtype hint for UI rendering. See §4.              |
| `x-*`         | any                 | User or tool extensions. See §3.4.                  |

Any field not listed above is ignored by the core but preserved on
write. Round-tripping must not lose unknown fields.

### 3.3 `id` rules

- Lowercase ASCII, digits, and `-`. Matches `^[a-z0-9][a-z0-9-]{0,63}$`.
- Unique within the vault.
- Stable: editors must not change an `id` on rename. The filename may
  change; the `id` is authoritative for links and audit.
- If `id` is omitted on write, the core derives it from the filename.

### 3.4 Extensions (`x-*`)

Third-party tools may add fields prefixed with `x-`. They are
preserved on round-trip but are never consulted by the core. This is
the stable extension point; do not repurpose reserved fields.

---

## 4. Seed types

Seed types ship with the desktop UI's form hints. The `schema` field
may narrow a type (e.g. `type: relationship`, `schema: colleague`).
Schemas are advisory only; the core does not reject unknown schemas.

| Type            | Purpose                                                  | Common `schema` values                  |
|-----------------|----------------------------------------------------------|-----------------------------------------|
| `identity`      | Who the user is: name, pronouns, background, bio.        | `profile`, `bio`                        |
| `roles`         | Roles and responsibilities held by the user.             | `job`, `volunteer`, `family`            |
| `goals`         | Current and past goals, with target dates and status.    | `goal`, `objective`, `milestone`        |
| `relationships` | People and organizations in the user's life.             | `colleague`, `manager`, `friend`, `family`, `org` |
| `memories`      | Notable events, experiences, anecdotes.                  | `event`, `anecdote`                     |
| `tools`         | Software, services, and systems the user relies on.      | `app`, `service`, `hardware`            |
| `preferences`   | Communication style, working preferences, constraints.   | `communication`, `working-style`, `constraint` |
| `domains`       | Domain knowledge, expertise areas, references.           | `field`, `reference`                    |
| `decisions`     | Significant decisions and their rationale.               | `decision`, `policy`                    |

New seed types require a spec version bump (§8).

---

## 5. Visibility and permission

`visibility` is a string label. It is the atom of the permission
system: agent tokens grant access to one or more visibility labels.

Built-in labels:

- `public` — content the user is comfortable sharing broadly.
- `work` — professional context. Typical default for work agents.
- `personal` — non-work life context the user treats as private to
  themselves and in-app tools, but is willing to share with
  appropriately-scoped agents.
- `private` — **hard floor.** Never included in a grant unless the
  grant explicitly names `private`. The desktop app shows a distinct
  warning when a user approves a scope that includes `private`. This
  is the label for notes the user never wants to leak by accident
  (health, finances, therapy, relationships).

Users may define any additional label (e.g. `medical`, `finance`).
Labels are free-form strings matching `^[a-z][a-z0-9-]*$`.

A document has exactly one `visibility`. If finer-grained sharing is
needed, split the document.

### 5.1 The `private` floor

Implementations must enforce these rules for any document whose
`visibility` is `private`:

- A token's scope must contain the literal string `private` for the
  document to be visible. No substring or pattern match.
- Out-of-scope `private` documents are indistinguishable from
  non-existent ones in all error responses (same `not_authorized`
  error, same listing behavior).
- The desktop app surfaces any grant that includes `private` with a
  distinct visual warning at approval time and in the token list.
- User-defined labels do not inherit the hard-floor rule — only the
  built-in `private` label does.

---

## 6. Links

Ourtex uses Obsidian-style wikilinks for inter-document references.

Syntax:

```
[[id]]
[[id|display text]]
[[id#section]]
[[id#section|display text]]
```

- `id` must resolve to an existing document's `id` or `alias`.
- `#section` targets a markdown heading in the target body.
- Unresolved links are allowed (a user may link ahead); the indexer
  surfaces them as "dangling".

The `links` frontmatter field is the **authoritative** set of outbound
references. Links in the body are discovered and reconciled by the
indexer but are not authoritative. When the user writes a new body
link, the editor adds it to `links`; when the user removes one, the
editor removes it.

Backlinks are derived by the indexer and never stored in frontmatter.

---

## 7. The `.ourtex/` directory

Reserved for Ourtex's internal state. Users should not edit files here
by hand. Syncing tools should include it (so permissions and audit
travel with the vault), unless the user explicitly opts out.

```
.ourtex/
├─ config.json         user preferences, driver selection, UI state
├─ tokens.json         hashed agent tokens + scopes + metadata
├─ audit.log           append-only, hash-chained
├─ index.sqlite        derived search + graph index (safe to delete)
├─ proposals/          pending agent-proposed writes
├─ keys/               (v2) encrypted key material
└─ version             single-line vault format version, e.g. "0.1"
```

`index.sqlite` is fully derived from the vault contents. Deleting it
triggers a full reindex on next launch. No authoritative data lives
there.

---

## 8. Versioning

The vault format is versioned with a single integer.minor pair (this
document describes `0.1`). The version is written to `.ourtex/version`
on vault creation.

- **Patch-level** changes (new optional fields, new seed `schema`
  values) do not bump the version.
- **Minor** bumps add new seed types or new required optional-field
  semantics. Readers of an older minor must still open newer vaults,
  ignoring unknown content.
- **Major** bumps are reserved for breaking changes and should be
  avoided. A migration tool ships with any major bump.

The core refuses to write to a vault whose `version` is newer than it
understands, to avoid downgrading unknown content.

---

## 9. Example documents

### 9.1 Identity

```markdown
---
id: me
type: identity
schema: profile
visibility: personal
tags: [core]
created: 2026-04-18
updated: 2026-04-18
---

# About me

I'm a product manager based in Toronto. I work in B2B SaaS and am
currently focused on developer tools. I prefer written async
communication over meetings.
```

### 9.2 Goal

```markdown
---
id: goal-q2-launch
type: goal
schema: objective
visibility: work
tags: [q2-2026, launch]
links: [[rel-jane-smith]]
created: 2026-04-01
updated: 2026-04-18
---

# Q2 launch of Ourtex public beta

**Target:** 2026-06-30
**Status:** on track

Ship the desktop app with local MCP server, seed types, and Obsidian
import. Reviewed weekly with [[rel-jane-smith]].
```

### 9.3 Preference

```markdown
---
id: pref-comms
type: preferences
schema: communication
visibility: work
tags: [style]
created: 2026-04-18
updated: 2026-04-18
---

# Communication style

- Prefer written over spoken.
- Short and direct; no preamble.
- Bullet points over paragraphs for status updates.
- Flag uncertainty explicitly ("~70% confident").
```

---

## 10. Non-goals

The format deliberately does not define:

- Binary attachments. Users may place binaries in the vault, but the
  core does not index or sync them in v1.
- A query language. Search and filtering are UI concerns, not format
  concerns.
- Embedded computation or templates. Documents are inert.
- A rich-text representation. Markdown is authoritative; rendering is
  a view concern.

These may be added in later spec versions if demand is real and the
design is clearly within the guiding principles.

---

## 11. Planned additions for Phase 2

These are not part of v0.1. They are sketched here so that vaults
written today round-trip cleanly when Phase 2 ships.

### 11.1 Workspace registry (Phase 2a)

The desktop app tracks a user's registered workspaces in a single
per-user file at `~/.ourtex/workspaces.json` — **outside** any vault.
This file is not part of the vault format; it is per-install client
state. A vault root never references the registry. A vault remains
portable: drop it onto another machine, register it as a new
workspace, and it opens without edits.

### 11.2 Team seed type `org/` (Phase 2c)

Team workspaces add a new seed type `org/` for business context
(goals, marketing stance, tone, policies) shared across members.
Frontmatter semantics are unchanged; `type: org` is what the indexer
and UI key on.

`org/` earns a spec bump to v0.2 when it ships. Older readers opening
a v0.2 vault treat `org/` as a custom type — they see the documents,
just without the team-aware UI hints.

### 11.3 Built-in visibility `org` (Phase 2c)

A new built-in visibility label `org` is added for documents visible
to all members of a team workspace, regardless of role.

- Like `private`, `org` is a **hard label**: scope must contain the
  literal string `org` to surface any `org`-labelled document. No
  implicit promotion.
- Unlike `private`, `org` is meaningful only inside a team workspace;
  in a personal vault it behaves as a plain user-defined label.
- Admin-only write (Phase 2c decision D10).

### 11.4 `principal` field in team workspaces

The reserved `principal` field (§3.2) starts carrying meaningful
values in team workspaces: the ID of the account (member) or team
that owns the document. The field already round-trips today; Phase 2c
just starts populating it.

No format change is required for 11.1 or 11.4. 11.2 and 11.3 are the
v0.2 delta.
