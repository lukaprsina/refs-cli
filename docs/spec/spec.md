# `refs` — local reference repos for coding agents

**Status:** MVP spec. Ready to implement. Vocabulary: `CONTEXT.md`. Architecture decisions: `docs/adr/`.
**Names:** crate `refs-cli`, binary `refs`, files `refs.toml` / `refs.lock`, block markers `BEGIN:refs` / `END:refs`.
**Language:** Rust, single static binary.

---

## 1. What we are building

A CLI that clones the git repos a project depends on (libraries, frameworks, their docs repos), checks out only the parts that matter at a pinned commit into `.references/`, and writes a **short block** into `AGENTS.md`: what exists, which packages each repo documents, and where to start. The agent then uses its own native tools (read, grep, glob) on real files.

There is no MCP server, no query API and no generated file index. The product is the pinned checkouts plus the block that makes the agent use them.

### The core rule

> One hop to the block (what exists, which packages, where to start); two hops to the files.

- **One hop:** the managed block in `AGENTS.md`, always in context. Per repo: id, ref, short SHA, a description, the npm-style packages it documents, and start paths.
- **Two hops:** the files under `.references/<repo>/`.

---

## 2. Background and evidence (why this shape)

Do not re-litigate this in code; keep it in mind when naming things and writing the block text.

### 2.1 Vercel: passive context beats on-demand retrieval

Vercel's eval post ("AGENTS.md outperforms skills in our agent evals", Jan 2026) tested Next.js 16 APIs that are not in model training data:

| Configuration | Pass rate |
|---|---|
| Baseline, no docs | 53% |
| Skill, default behaviour | 53% (skill not invoked in 56% of cases) |
| Skill + explicit "use the skill" instruction in AGENTS.md | 79% |
| Compressed docs index in AGENTS.md | 100% |

Takeaways we adopt:

- **No decision point.** Anything the agent must *decide* to call (skill, MCP tool, CLI) will often go uncalled. Information already in `AGENTS.md` needs no decision.
- **One instruction does a lot:** "Prefer retrieval-led reasoning over pre-training-led reasoning."

Caveat: one vendor, one framework that models already know changes often. Vercel favoured an inline index; our own evals (below) did not reproduce a benefit from one.

### 2.2 Matt Pocock (AI Hero): keep AGENTS.md small

"A Complete Guide To AGENTS.md" argues for a minimal AGENTS.md: everything loads on every request, frontier models follow roughly 150–200 instructions reliably, stale file paths poison context, and auto-generated files favour completeness over restraint. Our block is reference data plus one instruction, is regenerated from a pinned commit on every `sync` (so it can't go stale), and is small by construction.

### 2.3 Our own evals (solid-drums, Sonnet 5.5; run-by-run findings are kept privately)

- Stage-2 per-repo index files: 1 incidental grep in 11 runs, even when the block had no tree. Dropped.
- Header-only blocks (no inline tree) got as many `.references` calls as tree blocks, aimed at the right files. The tree is dropped.
- All searching went through Bash (`ls`/`rg`/`sed`); no run used the Grep or Glob tools.
- The recurring bug (`action()` called without `useAction`) came from same-name APIs in different packages (solid-js core vs `@solidjs/router`). Only reading the right package's source avoided it. Hence the `Packages:` field and the lookup sentence.
- Whole-app runs are a poor instrument (expensive; the target behaviour appears in about half of them).

### 2.4 Prior art and how we differ

| Tool | Shape | Why it is not this |
|---|---|---|
| Context7 (Upstash) | Hosted index; LLM-extracted snippets; MCP or `ctx7` CLI + skill | Not local. Pre-cut snippets, not files. Freshness depends on their re-indexing. Free tier ~1000 calls/month. Tool call = decision point. Complementary. |
| btca (Ben Davis) | CLI that clones repos; `ask` hands the question to a sub-agent; `btca reference` clones to `./references/` and prints an AGENTS.md snippet | Delegates to a sub-agent instead of informing the main agent. No pinning, no generated block. |
| Grounded Docs | Scrapes/indexes docs into SQLite + embeddings, MCP + CLI search | Chunks, not files. Tool call = decision point. |
| Vercel `@next/codemod agents-md` | Downloads matching Next docs, injects compressed index | Next.js only. This is the thing we generalise. |
| The author's previous setup | Git submodules in `vendor/` + a note in AGENTS.md | Agent only used them when told. No pinning tool, no retrieval instruction, no package mapping. |

---

## 3. Scope

### 3.1 In the MVP

- TOML config (project, plus a global template library) with groups and repos.
- Lock file pinning each repo to a commit SHA.
- Global cache of blobless bare clones; per-project sparse `git worktree` checkouts in `.references/<repo>/`.
- Managed block in `AGENTS.md`: header line per repo with `Packages:` and `Start:`.
- `.git/info/exclude` rule for the references directory.
- `enabled` flag on groups and repos: a disabled repo behaves as absent for lock, checkouts and the block (§4, §6.5).
- CLI: `init`, `add`, `remove`, `enable`, `disable`, `list`, `lock`, `sync`, `doctor`.

### 3.2 Deferred (rough priority order)

1. `add` infers `packages` from `package.json` `name`s under `paths`.
2. An optional inline tree as an add-on to the header lines (Vercel favoured one; revisit if evals show a gap).
3. Per-file hints (titles, export names), if evals show a need. Markdown/MDX via a real parser (`pulldown-cmark` or `markdown`), never regex scraping.
4. Linter/formatter exclusion: generating ignore entries for common tools. In the MVP this is the user's job; `init`/`doctor` say so.
5. Ref resolution from package lockfiles (`package.json`/`pnpm-lock.yaml` version → git tag).
6. Injecting a block for global-only repos outside any project.
7. `--json` output, `gc`/`clean` for the global cache.

### 3.3 Considered and discarded (with reasons)

| Idea | Why discarded |
|---|---|
| **MCP server** or **query CLI as the primary interface** | Structurally a skill: a decision point the agent often skips (Vercel: 53% with skill uninvoked). |
| **Budgeted inline file tree** (`inline_budget`, water-filling, truncation markers) | Header-only blocks matched tree blocks in evals (runs e, f). Cost: a whole algorithm and a churning block. May return as an optional add-on. |
| **Per-repo `.references/<repo>.md` index files** | 1 incidental read in 11 runs, even with no tree. Agents `ls`/`grep` the checkouts instead. |
| **Global `use` (project enabling groups from the global config)** | The committed block would depend on machine-local config: teammates and CI render a different block and `sync --check` fails forever. Global config is a template library instead; `refs add <id>` copies (§6.2). |
| **`docs`/`code`/`examples` path kinds** | Existed only to drive indexing. Now one flat `paths` list. |
| **Auto-updating unpinned repos before answering a query** | Churns the committed block and makes results drift. Updates happen only via `refs lock --upgrade`. |
| **Serving content from the git object DB** | If files aren't on disk the agent needs our tool to read them: a decision point again. |
| **Tantivy / SQLite FTS / embeddings / RAG** | Not needed for files the agent reads directly. |
| **Symlinks from `.references/` into a shared cache** | ripgrep doesn't follow symlinks by default. Worktrees give real files with shared objects. |
| **Full (non-sparse) clones per project** | Wasteful; most repos only need `docs/` and a source dir. |
| **Committing checkouts / git submodules in `vendor/`** | Bloats the repo; in practice agents didn't use them. |
| **`.gitignore` containing `*` inside `.references/`** | ripgrep reads ignore files inside a directory it searches, so it would hide every checkout even from explicit searches. |
| **`.git/info/exclude` negation to commit a lock inside `.references/`** | Git can't re-include a file whose parent dir is excluded; `/.references/*` + `!…` changes which paths match. Not worth it. |
| **Config/lock inside `.references/`** | The config sets the directory's name; the lock must be committed while the directory is excluded. |
| **Boost ranking by project imports** | Makes the block depend on project code and churns AGENTS.md on unrelated commits. |
| **Regex heading scraping** | Broken by code fences, inline code, JSX. |

---

## 4. Concepts

- **Repo:** one upstream git repository at one ref, e.g. `solid`. A docs site in a separate repo (e.g. `solidjs/solid-docs`) is simply another repo entry, usually in the same group.
- **Group:** organisational unit. Groups are the headings of the managed block, and a group's `description` is the "when to look here" hint for the agent.
- **Paths:** per repo, one flat list of repo-relative directories. Their union is the sparse checkout. Absent = whole repo.
- **Packages:** optional per repo; the package names (as imported in code, e.g. `@solidjs/router`) this repo documents or implements.
- **Start:** optional per repo; repo-relative files worth reading first (e.g. a migration guide).
- **Active:** a repo is active when it and its group are both enabled (`enabled`, default `true`). Only active repos are locked, checked out and rendered. Everything downstream of config parsing sees only the active set.
- **Ref:** branch, tag, or full commit SHA. Omitted → `settings.default_ref`, default `"HEAD"` (the remote's default branch).
- **Lock:** the resolved commit SHA for each repo, committed to the project.
- **Checkout:** a sparse, detached-HEAD `git worktree` at `.references/<repo>/`.
- **Managed block:** the generated section in `AGENTS.md` between markers.
- **Global library:** repo and group definitions in the global config, used only as templates by `refs add <id>`.

---

## 5. Files and locations

| File | Location | Committed? | Written by |
|---|---|---|---|
| Project config | `<project>/refs.toml` | yes | user, `init`, `add`, `remove` |
| Lock | `<project>/refs.lock` | yes | `lock`, `sync` |
| Global config (library) | `$XDG_CONFIG_HOME/refs/refs.toml` (default `~/.config/refs/refs.toml`) | n/a | user, `add --global`, `remove --global` |
| Global cache | `settings.cache_dir`, default `$XDG_CACHE_HOME/refs` (`~/.cache/refs`) | n/a | `lock`, `sync` |
| Checkouts | `<project>/<references_dir>/<repo>/`, default `.references/` | no (excluded) | `sync` |
| Managed block | `<project>/AGENTS.md` (configurable) | yes | `init`, `sync` |

Config and lock live at the **project root**, like `pyproject.toml`/`uv.lock`. The references directory stays entirely generated and entirely excluded.

**Project root discovery:** walk up from the current directory to the first directory containing `refs.toml`; if none, the git worktree root (for `init`); `--project <dir>` overrides.

---

## 6. Configuration

### 6.1 Schema

```toml
[settings]
cache_dir = "~/.cache/refs"        # global config only; warned about and ignored in project config
references_dir = ".references"     # project-relative; project config only
default_ref = "HEAD"               # project config only; "HEAD" = remote default branch
agents_files = ["AGENTS.md"]       # project config only; files that receive the managed block

[groups.<id>]
name = "Human name"                # required
description = "When to consult this group"   # optional, strongly recommended
enabled = true                     # optional, default true; false disables the group and all its repos

[repos.<id>]
url = "https://github.com/owner/repo"   # required; any URL git accepts, incl. file:// and SSH
group = "<group id>"                    # optional; ungrouped repos render under "Other"
ref = "main"                            # optional; branch, tag, or 40-char SHA
description = "One line on what this is" # optional, recommended
paths = ["packages", "documentation"]   # optional; sparse checkout (cone mode); absent = whole repo
packages = ["solid-js", "@solidjs/web"] # optional; rendered as "Packages:"
start = ["documentation/solid-2.0/MIGRATION.md"]  # optional; rendered as "Start:"
enabled = true                          # optional, default true; false = treated as absent (§6.5)
```

`<id>` for repos is the directory name under `.references/`; validate `[a-z0-9][a-z0-9._-]*`, not `.` or `..`, unique within the config.

Validation: `paths` entries are relative, no `..`; every `start` lies inside `paths` (when `paths` is present).

### 6.2 Global config is a template library

`sync` and `--check` read **only** the project's `refs.toml` and `refs.lock`. This is what makes the block reproducible on a teammate's machine and in CI.

- The global config holds `settings.cache_dir` and `[groups.*]` / `[repos.*]` definitions, in the same schema.
- `refs add <id>` (no URL) looks `<id>` up in the global library and **copies** the repo definition (and its group definition, if the project lacks that group) into the project's `refs.toml`. Later edits to the global library don't affect existing projects.
- `refs add --global <url> …` writes a definition to the global library only.
- Project `settings` are never read from the global config, apart from `cache_dir`.

### 6.3 Example (SolidJS 2.0, released Aug 2026, after every model's cutoff)

```toml
[groups.solidjs-2]
name = "SolidJS 2.0"
description = "Solid 2.0 release candidates, router and docs. Newer than your training data; many APIs differ from Solid 1.x."

[repos.solid]
url = "https://github.com/solidjs/solid"
group = "solidjs-2"
ref = "next"
description = "Solid 2.0 core packages and 2.0 design docs"
paths = ["packages", "documentation"]
packages = ["solid-js", "@solidjs/web", "@solidjs/signals"]
start = ["documentation/solid-2.0/MIGRATION.md"]

[repos.solid-router]
url = "https://github.com/solidjs/solid-router"
group = "solidjs-2"
ref = "next"
description = "Solid Router 2.0 source"
paths = ["src"]
packages = ["@solidjs/router"]
```

### 6.4 Editing

`add`/`remove`/`enable`/`disable` must preserve comments, ordering and formatting. Use `toml_edit` for writes; `serde` + `toml` for reads.

### 6.5 Disabled repos and groups

A disabled repo behaves as if it were commented out of the config, but stays visible to tooling. Use it to keep a reminder of where code came from (e.g. a proof-of-concept repo an agent already copied from) and switch it back on later.

- **Active set:** repo active ⇔ `repo.enabled` and its group's `enabled` are both true. Disabling a group cascades to every repo in it. An ungrouped repo depends only on its own flag.
- **Lock:** a disabled repo has no lock entry and isn't in `config_hash`. Disabling then running `lock`/`sync` drops its entry.
- **Checkout:** `sync` removes the checkout of a disabled repo like any repo no longer in the config (§7.3).
- **Block:** a disabled repo has no Entry. A group with no active repos renders no heading (this holds for enabled groups that simply have no repos too).
- **Re-enabling** re-resolves the ref, exactly as uncommenting would. A floating ref may land on a newer commit than before; the old pin isn't kept.
- **Validation still applies** to disabled entries (id, dangling group, `paths`, `start` syntax), but nothing is resolved or fetched for them. `list` shows them marked `disabled`; `doctor` reports how many are disabled (info).
- `refs disable <id>` / `refs enable <id>` set `enabled` on a repo; `--group` targets a group id instead. Both edit via `toml_edit`, then `sync` unless `--no-sync`. `enable` removes the key rather than writing `enabled = true`.

---

## 7. Storage and git mechanics

Shell out to the system `git` binary: it handles auth (credential helpers, SSH agents, private repos) for free. Spikes ran on git 2.55.0; determine and document the real minimum during implementation (`doctor` checks the version).

### 7.1 Global cache

- One **bare, blobless** clone per normalised URL: `<cache_dir>/git/<sha256(normalised_url)[..16]>/`, created with `git clone --bare --filter=blob:none <url>`. Store the original URL in a small metadata file beside it.
- URL normalisation: strip trailing `/` and `.git`, lowercase the host. Do not rewrite protocols.
- Guard every mutation of a cache repo with an exclusive file lock (e.g. `fd-lock`).
- **A bare clone has no fetch refspec** (verified): a plain `git fetch origin` fetches objects but updates no refs, only `FETCH_HEAD`. So never rely on refs in the cache. Resolve refs with `git ls-remote`; fetch by SHA (`git fetch origin <sha>`), falling back to fetching the ref if the server refuses SHA wants.
- `--filter=blob:none` works against a real remote (verified on GitHub, git 2.55.0): the clone downloads trees and commits only, `git fetch origin <sha>` works, and a sparse `checkout` downloads just the blobs it needs (10 → 4 missing blobs on a 10-blob test repo). A local `file://` server ignores the filter, so use a real remote when testing this.

### 7.2 Resolution (`lock`)

- Branch or tag or `HEAD`: `git ls-remote <url> <ref>` (for `HEAD`, the remote's symbolic HEAD). Prefer tags over branches if both match; error if still ambiguous.
- A 40-hex ref is taken as-is and verified on fetch. Abbreviated SHAs are rejected.
- For each configured path, verify it exists at the resolved SHA (`git ls-tree`). A missing path is an error naming repo, path and SHA.
- For each `start` path, verify it exists at the SHA and lies inside `paths`. A missing one is an error.

### 7.3 Checkouts (`sync`)

For each locked repo:

1. Ensure the cache clone has the SHA.
2. If `.references/<repo>/` doesn't exist: `git -C <cache> worktree add --no-checkout --detach <project>/<references_dir>/<repo> <sha>`, then `git -C <worktree> sparse-checkout set --cone <paths>` (skip when `paths` is absent), then `git -C <worktree> checkout --detach <sha>`.
3. If it exists at another SHA: update sparse patterns if paths changed, then `checkout --detach <sha>`.
4. If `.references/<repo>/` exists but isn't a worktree of the expected cache repo, stop with an error; never delete user data.

**Sparse config is per-worktree** (verified): the first `sparse-checkout set` enables `extensions.worktreeConfig` in the cache and writes patterns to `worktrees/<name>/config.worktree`. Two worktrees of one cache hold different patterns; nothing leaks across projects. The cache stays bare (`core.bare` moves to its own `config.worktree`).

Also:

- Remove worktrees for repos no longer active (removed from the config or disabled, §6.5): `git worktree remove --force` (generated, read-only copies).
- Run `git worktree prune` on each touched cache repo.
- Moving a project directory breaks worktree links; `sync` detects this and repairs with `git worktree repair` or recreates the checkout.
- Ignore submodules and Git LFS content in the MVP; `doctor` mentions them if detected.

### 7.4 Exclusion

- `sync` and `init` ensure `/<references_dir>/` is a line in `<git-dir>/info/exclude` (the common git dir for linked worktrees of the project). Per-clone, which is fine since every clone runs `sync`.
- If the project isn't in a git repo, warn and continue.
- Consequence we rely on: default searches (ripgrep) skip `.references/`, so a grep for `useEffect` finds only the user's code. When a search path is passed explicitly (e.g. `.references/solid`), ripgrep searches it anyway. Hidden entries inside a checkout (e.g. `.github/`) stay skipped.
- Linters, formatters and type checkers are **the user's responsibility** in the MVP. `init` and `doctor` print a reminder.

---

## 8. Block content

### 8.1 Header line

One line per repo:

```
[<id> @ <ref> <sha7>] <description>. Packages: <p>, <p>. Start: <path>, <path>.
```

- Omit empty fields (no description, no `Packages:`, no `Start:`). When the ref is a SHA, show `@ <sha7>` once.
- Only active repos render (§6.5). Repos render in config order within their group; groups in order of first definition; ungrouped repos last under `### Other`. A group with no active repos renders no heading.
- Header lines sit in a ```` ```text ```` fence per group, so Markdown formatters leave them alone (verified problem in the pilot: oxfmt rewrote `|` lines as a table). A blank line follows `<!-- BEGIN:refs -->`, which oxfmt requires.
- The block must be stable under common Markdown formatters (oxfmt, prettier, dprint), or `sync --check` fights `fmt --check`.

### 8.2 Determinism

Output depends only on (config, lock, generator version). Same inputs → byte-identical block. `sync` run twice is a no-op. `packages`, `start` and descriptions change only the rendered block, not the lock.

---

## 9. The managed block

### 9.1 Markers and placement

```
<!-- BEGIN:refs -->
…generated…
<!-- END:refs -->
```

- Only content between the markers is ever touched. If the markers are missing, `init`/`sync` appends the block at the end of the file (creating it if needed). Unbalanced, nested or duplicated markers: error, don't write.
- Written to each file in `settings.agents_files` (default `AGENTS.md`). Write only if the content changed.
- Claude Code loads `AGENTS.md` only when `CLAUDE.md` doesn't exist. `doctor` checks the relationship (§10.1) but `sync` never creates or modifies `CLAUDE.md` unless it's in `agents_files`.
- If `CLAUDE.md` is in `agents_files` **and** also imports `AGENTS.md` (`@AGENTS.md`) or is a symlink to it, the block loads twice: `doctor` flags it.

### 9.2 Content template

The exact wording is an eval variable and can change later. Keep the instruction count tiny and the tone plain: no "MUST", no capitals.

````markdown
<!-- BEGIN:refs -->

## Reference repos

Source and docs for some dependencies are checked out read-only in `.references/`, pinned in `refs.lock`. They can be newer than your training data. Prefer retrieval-led reasoning over pre-training-led reasoning for these libraries: read the relevant files before writing code that uses them. Look up an imported API in a repo whose `Packages:` lists the package it's imported from (e.g. imports from `<first package>` → `.references/<its repo id>`); APIs with the same name can behave differently in different packages.

`.references/` is excluded from default search. To search it, pass the path explicitly, e.g. `.references/<first repo id>`. Don't edit anything in `.references/`; it is regenerated by `refs sync`. If `.references/` is missing, tell the user to run `refs sync`.

### SolidJS 2.0

Solid 2.0 release candidates, router and docs. Newer than your training data; many APIs differ from Solid 1.x.

```text
[solid @ next ee49b3e] Solid 2.0 core packages and 2.0 design docs. Packages: solid-js, @solidjs/web, @solidjs/signals. Start: documentation/solid-2.0/MIGRATION.md.
[solid-router @ next 4658036] Solid Router 2.0 source. Packages: @solidjs/router.
```

<!-- END:refs -->
````

- `<first package>` is the first package of the first repo that has one. If no repo lists packages, omit the lookup sentence.
- Fixed prose is short; group descriptions come from config.

---

## 10. CLI

Semantics follow `uv`: config is intent, the lock is resolution, `sync` makes the disk match the lock. Nothing updates behind the user's back.

Global flags: `--project <dir>`, `-q/--quiet`, `-v/--verbose`, `--no-color`.
Exit codes: `0` ok, `1` error, `2` usage error, `3` `--check` found something out of date.

| Command | Behaviour |
|---|---|
| `refs init` | Create `refs.toml` (commented template) if absent, add the empty managed block to `agents_files`, add the exclude rule. Print the linter/formatter reminder and the CLAUDE.md note. Idempotent. |
| `refs add <url> [--id <id>] [--group <g>] [--ref <r>] [--paths <p>…] [--packages <p>…] [--start <p>…] [--description <d>] [--global] [--no-sync]` | Append a repo via `toml_edit`. Id defaults to the URL's last path segment. Creates the group stub if missing. Unless `--global` or `--no-sync`, runs `lock` for that repo and then `sync`. Accepts `https://github.com/o/r/tree/<ref>/<path>` and derives ref and a path from it. |
| `refs add <id>` | No URL: copy the definition (and group) for `<id>` from the global library into `refs.toml` (§6.2), then lock and sync as above. Error if `<id>` isn't in the library. |
| `refs remove <id> [--global] [--no-sync]` | Remove the repo entry, leaving comments and other entries alone. Remove an empty group only if it has no description. Then `sync` unless `--no-sync`. |
| `refs enable <id> [--group] [--no-sync]`, `refs disable <id> [--group] [--no-sync]` | Set or clear `enabled` on a repo, or on a group with `--group` (§6.5). Then `sync` unless `--no-sync`. |
| `refs list` | Repos grouped: id, url, requested ref, locked SHA (short), checkout state (ok / missing / wrong SHA / not locked / disabled). |
| `refs lock [--upgrade [<id>…]]` | Resolve refs → SHAs and write `refs.lock`. Without `--upgrade`, already-locked repos keep their SHA if their config entry is unchanged. With `--upgrade`, re-resolve floating refs (all, or the given ids). No checkouts touched. Also checks `start` paths (§7.2). |
| `refs sync [--check] [--locked] [--offline]` | See below. `--locked`: error instead of locking when the lock is missing or stale. `--check`: change nothing, exit 3 if the lock, checkouts or blocks are out of date (CI, pre-commit). `--offline`: cache only; error on missing objects. |
| `refs doctor` | Diagnose without changing anything (§10.1). |

`sync` steps:

0. Compute the active set from the config (§6.5). Steps 1–5 see only active repos.
1. Resolve refs (lock if missing or stale, unless `--locked`).
2. Fetch into the bare blobless cache.
3. Update the sparse worktrees; prune removed repos.
4. Check `start` paths.
5. Render the block; write it only if it changed.
6. Ensure the exclude rule.

### 10.1 `doctor` checks

- git present and new enough.
- Project and global config parse and validate: unknown keys, bad ids, dangling group refs, `cache_dir` set in project config, settings that only apply globally.
- `start` path missing or outside `paths`: error.
- A package listed under more than one active repo: info.
- Number of disabled repos and groups: info.
- Lock present and its `config_hash` matches.
- Each checkout exists, is a worktree of the right cache repo, is at the locked SHA, and has the right sparse patterns.
- Orphan directories in `references_dir` not matching any repo.
- Exclude rule present.
- Managed markers present and well-formed in each `agents_files` entry.
- `CLAUDE.md` vs `AGENTS.md`:
  - `CLAUDE.md` exists, `AGENTS.md` holds the block, and `CLAUDE.md` neither imports (`@AGENTS.md`) nor symlinks to it: warn (Claude Code loads only `CLAUDE.md`).
  - `CLAUDE.md` in `agents_files` and also importing/symlinking `AGENTS.md`: warn (double load).
- Reminder that linters/formatters/type checkers may scan `references_dir`.
- Submodules or LFS pointers detected in a checkout: info.

Each finding is `ok` / `warn` / `error` with a one-line fix. Exit 1 if any error.

### 10.2 Lock file format

TOML, sorted by repo id, stable formatting:

```toml
# Generated by refs. Do not edit.
version = 1
config_hash = "sha256:…"   # hash of the normalised active repo set: (id, url, ref, paths)

[[repo]]
id = "solid"
source = "git"              # discriminator; other kinds add their own fields later
url = "https://github.com/solidjs/solid"
ref = "next"
sha = "1a2b3c4d5e6f…"      # full 40 hex
paths = ["packages", "documentation"]
```

The entry is a tagged union on `source`. `git` is the only kind in the MVP and keeps git-specific field names (`url`, `ref`, `sha`). Adding a kind is additive and needs no `version` bump. The same `source` key is accepted, optional and defaulting to `"git"`, on `[repos.<id>]` in `refs.toml`. Code outside the source module treats the resolved identity as an opaque pin.

`config_hash` covers only what affects resolution and checkout. Descriptions, group names, `packages` and `start` don't stale the lock; they only change the block on the next `sync`. The global config never enters the hash.

---

## 11. Implementation notes

- **Crates (suggested):** `clap` (derive), `serde` + `toml` (read), `toml_edit` (write), `anyhow`/`thiserror`, `fd-lock`, `sha2`, `etcetera` or `directories` (XDG paths), `similar` for `--check` diffs in verbose mode.
- **Git:** shell out via `std::process::Command`; no libgit2 or gix in the MVP. One module with a thin, testable wrapper. Set `GIT_TERMINAL_PROMPT=0` for non-interactive commands so CI fails fast.
- **Atomic writes** for `refs.lock` and AGENTS.md: temp file in the same directory, then rename.
- **Paths:** `camino` or careful `Path` handling; forward slashes in rendered output.
- **Windows:** not a target for the MVP; avoid gratuitous Unix-only assumptions.

### 11.1 Milestones

1. **Git layer:** cache, `lock`, sparse worktrees, exclude rule. Spikes are done (§7). Acceptance: the git lifecycle test (§12) passes.
2. **Block renderer plus `sync`**, with golden tests, including formatter stability.
3. **`init`, `add`, `remove`, `list`** via `toml_edit`, including `add <id>` from the global library.
4. **`doctor`.**

---

## 12. Tests

- **Golden block:** fixture repos built in tests as local `file://` bare repos; snapshot the rendered block (e.g. `insta`). Cover missing fields, SHA refs, ungrouped repos, no packages anywhere.
- **Formatter stability:** run oxfmt / prettier / dprint over a file containing the block; output must be unchanged.
- **Idempotence:** `sync` twice → second run writes nothing (`--check` exits 0).
- **Round-trips:** `add` then `remove` leaves `refs.toml` byte-identical, including comments.
- **Git lifecycle:** add → lock → sync → change ref → lock --upgrade → sync → remove → sync; moved project dir → sync repairs; two projects sharing one cache repo with different sparse paths; concurrent syncs.
- **Reproducibility:** `sync --check` passes on a machine with no global config.
- **Block safety:** content outside markers untouched; malformed markers refuse to write.
- **`start` validation:** missing path and outside-`paths` errors.
- **Enabled flag:** disabling a repo drops its lock entry, checkout and Entry; disabling a group cascades; a group with no active repos has no heading; `disable` then `enable` round-trips `refs.toml` byte-identical; disabled entries still fail validation on bad ids and dangling groups.

---

## 13. Evaluation plan (after the MVP works)

Out of scope for building the CLI; recorded so the evals can resume.

- Narrow tasks that force the target behaviour (e.g. "add save/delete using `@solidjs/router` actions"), block vs no block, at least 2 runs each. Whole-app runs cost about 2–3M input tokens each and hit the target in about half of them.
- Judge from transcripts, not self-reports; check `message.model`; pin the model and use an identical kickoff message with no mid-run steering.
- Variables: lookup-sentence wording; header-only vs an optional inline tree; the missing-checkout line.

---

## 14. Open questions

1. Minimum git version (spikes ran on 2.55.0).
2. Final wording of the block, to be settled by evals.

Resolved: a package listed under several repos is rendered as written (each repo's line lists it) and `doctor` reports it as info. A repo has exactly one group; there is no multi-group membership (a group is only a block heading plus a "when to look here" description, and lookup goes through `Packages:`).
