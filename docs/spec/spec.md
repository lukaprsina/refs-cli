# `refs` — local reference repos for coding agents

**Status:** MVP spec. Vocabulary: `CONTEXT.md`. Structure: `docs/architecture.md`. Decisions: `docs/adr/`.
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

- Project TOML config with groups and repos.
- Lock file pinning each repo to a commit SHA.
- Global cache of blobless bare clones; per-project sparse `git worktree` checkouts in `.references/<repo>/`.
- Managed block in `AGENTS.md`: header line per repo with `Packages:` and `Start:`.
- `.git/info/exclude` rule for the references directory.
- `enabled` flag on groups and repos: a disabled repo behaves as absent for lock, checkouts and the block (§4, §6.4).
- CLI: `init`, `add`, `remove`, `enable`, `disable`, `list`, `lock`, `sync`, `doctor`.

### 3.2 Deferred (rough priority order)

1. `add` infers `packages` from `package.json` `name`s under `paths`.
2. An optional inline tree as an add-on to the header lines (Vercel favoured one; revisit if evals show a gap).
3. Per-file hints (titles, export names), if evals show a need. Markdown/MDX via a real parser (`pulldown-cmark` or `markdown`), never regex scraping.
4. Linter/formatter exclusion: generating ignore entries for common tools. In the MVP this is the user's job; `init`/`doctor` say so.
5. Ref resolution from package lockfiles (`package.json`/`pnpm-lock.yaml` version → git tag).
6. `--json` output, `gc`/`clean` for the global cache.

### 3.3 Considered and discarded (with reasons)

| Idea | Why discarded |
|---|---|
| **MCP server** or **query CLI as the primary interface** | Structurally a skill: a decision point the agent often skips (Vercel: 53% with skill uninvoked). |
| **Budgeted inline file tree** (`inline_budget`, water-filling, truncation markers) | Header-only blocks matched tree blocks in evals (runs e, f). Cost: a whole algorithm and a churning block. May return as an optional add-on. |
| **Per-repo `.references/<repo>.md` index files** | 1 incidental read in 11 runs, even with no tree. Agents `ls`/`grep` the checkouts instead. |
| **Global template library** | It adds a second config location, global add/remove behavior, and copy semantics for groups. Projects would still need their own explicit repo definitions for reproducible blocks, so users can add repos directly to each project's config in the MVP. |
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
- **Start:** optional per repo; repo-relative files worth reading first (e.g. a migration guide). Each must be in the checkout: inside a `paths` entry, or (cone mode always includes these, verified) a file directly in the repo root or directly in an ancestor directory of a `paths` entry.
- **Active:** a repo is active when it and its group are both enabled (`enabled`, default `true`). Only active repos are locked, checked out and rendered. Everything downstream of config parsing sees only the active set.
- **Ref:** branch, tag, or full commit SHA. Omitted → `settings.default_ref`, default `"HEAD"` (the remote's default branch).
- **Lock:** the resolved commit SHA for each repo, committed to the project.
- **Checkout:** a sparse, detached-HEAD `git worktree` at `.references/<repo>/`.
- **Managed block:** the generated section in `AGENTS.md` between markers.

---

## 5. Files and locations

| File | Location | Committed? | Written by |
|---|---|---|---|
| Project config | `<project>/refs.toml` | yes | user, `init`, `add`, `remove` |
| Lock | `<project>/refs.lock` | yes | `lock`, `sync` |
| Global cache | `$XDG_CACHE_HOME/refs` (default `~/.cache/refs`) | n/a | `lock`, `sync` |
| Checkouts | `<project>/<references_dir>/<repo>/`, default `.references/` | no (excluded) | `sync` |
| Managed block | `<project>/AGENTS.md` (configurable) | yes | `init`, `sync` |

Config and lock live at the **project root**, like `pyproject.toml`/`uv.lock`. The references directory stays entirely generated and entirely excluded.

**Project root discovery:** walk up from the current directory to the first directory containing `refs.toml`; if none, the git worktree root (for `init`); `--project <dir>` overrides. `init` run below a directory that already has a `refs.toml` warns and stops instead of silently using the parent; `init --here` creates a nested project in the current directory.

---

## 6. Configuration

### 6.1 Schema

```toml
[settings]
references_dir = ".references"     # project-relative; project config only
default_ref = "HEAD"               # project config only; "HEAD" = remote default branch
agents_files = ["AGENTS.md"]       # project config only; files that receive the managed block

[groups.<id>]
name = "Human name"                # required; letters, digits, spaces and . , : ( ) / + & - only (rendered as a heading)
description = "When to consult this group"   # optional, strongly recommended
enabled = true                     # optional, default true; false disables the group and all its repos

[repos.<id>]
url = "https://github.com/owner/repo"   # required; https, ssh, git or file://; no password, no leading "-"
group = "<group id>"                    # optional; ungrouped repos render under "Ungrouped"
ref = "main"                            # optional; branch, tag, or 40-char SHA; no leading "-"
description = "One line on what this is" # optional, recommended
paths = ["packages", "documentation"]   # optional; directories; sparse checkout (cone mode); absent = whole repo
packages = ["solid-js", "@solidjs/web"] # optional; rendered as "Packages:"
start = ["documentation/solid-2.0/MIGRATION.md"]  # optional; rendered as "Start:"
enabled = true                          # optional, default true; false = treated as absent (§6.4)
```

`<id>` for repos is the directory name under `.references/`; validate `[a-z0-9][a-z0-9._-]*`, not `.` or `..`, unique within the config.

Validation: `paths` entries are relative, no `..`; when `paths` is present, every `start` is inside a `paths` entry or a direct child of the root or of an ancestor of a `paths` entry (cone mode checks those files out too). With `paths = ["docs/guide"]`, `README.md`, `docs/README.md` and `docs/guide/a.md` are valid `start` values; `src/x.rs` and `docs/guide/../x.md` are not.

`settings.references_dir` and each `settings.agents_files` entry are project-relative output paths. Reject absolute paths, paths containing `..`, empty paths, and paths that resolve to the project root. For an existing destination or its nearest existing ancestor, resolve symlinks and require the resolved path to remain inside the canonical project root; reject broken symlinks. Symlinks that resolve inside the project are allowed. If the destination already exists, `references_dir` must be a directory and each `agents_files` target must be a regular file. This validation is not race-resistant against a symlink being changed between validation and writing.

Every string that is rendered into the block (`name`, `description` on groups and repos, `packages` and `start` entries) is a single line: no control characters (including newlines), no ```` ``` ````, and neither `BEGIN:refs` nor `END:refs`. Otherwise a config value could end the fence or the markers early.

Free text is rendered inside the `text` fence (§8.1), where formatters leave it alone. The one exception is a group `name`, which is the `###` heading: allow only Unicode letters and digits, spaces and `. , : ( ) / + & -`, not starting or ending with a space, so a formatter has nothing to rewrite.

**`url` and `ref` come from a file that may belong to an untrusted repository**, and `refs sync` hands them to git:

- Reject a `url` or `ref` that starts with `-` (git would read it as an option), and pass `--` or `--end-of-options` before them on every git command (§7.7).
- Reject a password in the URL userinfo (`https://user:token@host/…`): it would be committed in `refs.toml` and `refs.lock` and copied into the cache metadata. The diagnostic points at credential helpers and SSH agents. A bare username (`git@github.com:o/r`) is fine.
- `paths` entries name directories. §7.2 checks that each is a tree at the resolved SHA, not just that it exists.

**Parsing constraints:**

- **Order.** §8.1 renders repos in config order and groups in order of first definition, but `toml` sorts table keys alphabetically by default (verified, `toml` 1.1.6, even when deserializing into `IndexMap`). Enable the `preserve_order` feature on `toml` and use `indexmap::IndexMap` for `groups` and `repos`. Never `BTreeMap` for an ordered map: it sorts regardless. The §12 golden tests depend on this.
- **Spans.** Diagnostics need source spans (ADR 0002). A repo id is a table key, so key the map with `toml::Spanned<String>` (`IndexMap<Spanned<String>, Repo>`) to get the id's span. `Spanned` on a field gives no span for the key. List fields are `Vec<Spanned<String>>` (one span per element), not `Spanned<Vec<String>>` (whole-array span only); use this for `paths` and `start`.
- **Unknown keys.** `#[serde(deny_unknown_fields)]` yields a `toml::de::Error` whose `span()` points at the offending token; no hand-rolled check is needed.

### 6.2 Example (SolidJS 2.0, released Aug 2026, after every model's cutoff)

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

### 6.3 Editing

`add`/`remove`/`enable`/`disable` must preserve comments, ordering and formatting. Use `toml_edit` for writes; `serde` + `toml` for reads.

### 6.4 Disabled repos and groups

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

Shell out to the system `git` binary: it handles auth (credential helpers, SSH agents, private repos) for free. Spikes ran on git 2.55.0. **Minimum git is 2.36.0** (§7.5); `doctor` checks the version.

### 7.1 Global cache

- One **bare, blobless** clone per normalised URL: `<cache_root>/git/<sha256(normalised_url)[..16]>/`, where `<cache_root>` is `$XDG_CACHE_HOME/refs` (default `~/.cache/refs`). Create it with `git clone --bare --filter=blob:none <url>`. Store the original URL in a small metadata file beside it.
- URL normalisation: strip trailing `/` and `.git`, lowercase the host. Do not rewrite protocols: `https://github.com/o/r` and `git@github.com:o/r` get separate cache clones. Unifying them would mean guessing each host's URL mapping.
- Guard every mutation of a cache repo with an exclusive file lock (e.g. `fd-lock`).
- **Blobless is not shallow.** The clone and each fetch still bring every commit and tree reachable from the requested object. That is small for the repos this tool targets and large for something like `torvalds/linux` (hundreds of MB). Accepted for the MVP; a shallow or `tree:0` mode is a later option.
- **A bare clone has no fetch refspec** (verified): a plain `git fetch origin` fetches objects but updates no refs, only `FETCH_HEAD`. So never rely on refs in the cache. Resolve refs with `git ls-remote`; fetch by SHA (`git fetch origin <sha>`), falling back to fetching the ref if the server refuses SHA wants.
- `--filter=blob:none` is verified against a real remote only (GitHub, git 2.55.0): the clone downloads trees and commits only, `git fetch origin <sha>` works, and a sparse `checkout` downloads just the blobs it needs (10 → 4 missing blobs on a 10-blob test repo). A local `file://` server does not honour the filter: git prints `warning: filtering not recognized by server, ignoring` and silently makes a full clone. A `file://` fixture therefore cannot stand in for a real remote when testing this. Test the blobless path against a `git daemon` fixture with `uploadpack.allowFilter=true`, which runs in CI and honours the filter, and keep one opt-in test against a real remote (GitHub) for server differences (§12).

### 7.2 Resolution (`lock`)

- **Branch or tag:** one round trip with explicit patterns: `git ls-remote <url> refs/heads/<ref> refs/tags/<ref> 'refs/tags/<ref>^{}'`. The `^{}` pattern matches literally and returns the peeled commit line for an annotated tag (nothing extra for a lightweight one). Prefer tags over branches if both match; error if still ambiguous. Branch/tag name collisions are common in the repos people reference, so this is a real path.
- **`HEAD`:** `git ls-remote --symref <url> HEAD` returns the target branch and its SHA. The lock records the branch name (`branch`, §10.2) so the block can show `@ main` instead of `@ HEAD` (§8.1). If the server sends no `ref:` line (detached remote HEAD), `branch` is omitted.
- **Prefer targeted patterns over a bare `ls-remote`**: a bare listing is ~2000 refs on typical targets (`solidjs/solid` 1967, `torvalds/linux` 3828, ~0.8s each).
- **The lock stores the commit SHA, never a tag object SHA.** For an annotated tag, plain `ls-remote <url> <ref>` returns the tag object and no `^{}` line, and `worktree add` and `ls-tree` peel silently, so a tag SHA in the lock would check out a different commit than the one locked. `sync --check` would then fail forever and §8.1 would render an unlookable SHA, with no error anywhere. Resolve to the commit upstream of the lock.
- **Fallback:** peeled lines depend on the server advertising them (some proxies and dumb HTTP do not). If `^{}` is missing for a tag, fetch the tag object SHA, peel it locally with `git -C <cache> rev-parse <sha>^{commit}`, and store the resulting commit SHA in the lock.
- A 40-hex ref is taken as-is and verified on fetch. Abbreviated SHAs are rejected. Only 40-hex (SHA-1) object ids are supported; SHA-256 repositories (64 hex) are out of scope for the MVP.
- For each configured path, verify it exists at the resolved SHA and is a tree (`git ls-tree`). A missing path, or a path that is a file, is an error naming repo, path and SHA.
- For each `start` path, verify it exists at the SHA and will be in the checkout (inside `paths`, or a direct child of the root or of an ancestor of a path; §4). A missing one is an error.

### 7.3 Checkouts (`sync`)

For each locked repo:

1. Ensure the cache clone has the SHA.
1b. Prefetch the blobs the checkout will need (the `paths` union, or the whole tree), under the cache lock, so all network access happens in steps 1 and 1b and `checkout` never fetches lazily. With `--offline`, verify those blobs are present (`git rev-list --objects --missing=print`) and error if any are missing; run `checkout` with `GIT_NO_LAZY_FETCH=1` as a backstop.
2. If `.references/<repo>/` doesn't exist: run `git -C <cache> worktree prune` first (a deleted checkout, e.g. after `git clean -fdx`, leaves a stale registration that makes `worktree add` fail), then `git -C <cache> worktree add --no-checkout --detach <project>/<references_dir>/<repo> <sha>`, then `git -C <worktree> sparse-checkout set --cone <paths>` (skip when `paths` is absent), then `git -C <worktree> checkout --detach <sha>`.
3. If it exists at another SHA: update sparse patterns if paths changed, then `checkout --detach <sha>`.
4. If `.references/<repo>/` exists but isn't a worktree of the expected cache repo, stop with an error; never delete user data.

**Sparse config is per-worktree** (verified): the first `sparse-checkout set` enables `extensions.worktreeConfig` in the cache and writes patterns to `worktrees/<name>/config.worktree`. Two worktrees of one cache hold different patterns; nothing leaks across projects. The cache stays bare (its top-level `core.bare = true` is untouched).

Also:

- Remove worktrees for repos no longer active (removed from the config or disabled, §6.4): `git worktree remove --force` (generated, read-only copies). **Unless the checkout is dirty**: if `git status --porcelain` in it shows any modified or untracked file, `sync` refuses to remove it or move it to another SHA and reports `refs::sync::dirty_checkout` naming the files, so the "never delete user data" rule of step 4 also holds for edits an agent made despite the block. Untracked files count (a `node_modules` created by running a tool inside a checkout blocks `sync` too); the diagnostic says so and names `sync --force`, which discards them.
- Run `git worktree prune` on each touched cache repo. Prune forgets registrations whose directory is missing, including projects on a currently unmounted drive; their checkouts are recreated by the next `sync`. Accepted; `doctor` reports such registrations as info (§10.1).
- Moving a project directory breaks worktree links; `sync` detects this and repairs with `git worktree repair` or recreates the checkout.
- Ignore submodules and Git LFS content in the MVP; `doctor` mentions them if detected.

### 7.4 Server prerequisites

`--filter=blob:none` needs `uploadpack.allowFilter` on the server; fetch-by-SHA needs protocol v2 (default since git 2.26) or `uploadpack.allowReachableSHA1InWant`. GitHub and GitLab satisfy both; a self-hosted `git-http-backend` may not. When the server ignores the filter, `doctor` reports it as info (§10.1).

### 7.5 Minimum git version: 2.36.0

The floor is set by a bug fix, not a new command:

| Need | Version |
|---|---|
| `git sparse-checkout` + `--cone` | 2.25.0 |
| Protocol v2 default (helps fetch-by-SHA) | 2.26.0 |
| `git worktree repair` (§7.3, moved project dir) | 2.30.0 |
| **`sparse-checkout` per-worktree config works on a worktree of a bare repo** | **2.36.0** |

Before 2.36.0, `sparse-checkout set` in a worktree of a bare repo enabled `extensions.worktreeConfig` without relocating `core.bare`, so the worktree read `core.bare=true` from the shared config and treated itself as bare. That is exactly this architecture (bare blobless cache plus linked sparse worktrees), so it is broken below 2.36.

2.36.0 is April 2022 and fine on every current distro, but excludes **Ubuntu 22.04 (git 2.34.1)**. Document the exclusion; do not hand-roll a workaround (moving `core.bare` into `config.worktree` after `worktree add`) for a release past standard support.

### 7.6 Exclusion

- `sync` and `init` ensure `/<references_dir>/` is a line in `<git-dir>/info/exclude` (the common git dir for linked worktrees of the project). Per-clone, which is fine since every clone runs `sync`.
- If the project isn't in a git repo, warn and continue.
- Consequence we rely on: default searches (ripgrep) skip `.references/`, so a grep for `useEffect` finds only the user's code. When a search path is passed explicitly (e.g. `.references/solid`), ripgrep searches it anyway. Hidden entries inside a checkout (e.g. `.github/`) stay skipped.
- Linters, formatters and type checkers are **the user's responsibility** in the MVP. `init` and `doctor` print a reminder.

---

### 7.7 Running git on untrusted input

`refs.toml` may come from a repository the user just cloned, so `url` and `ref` are untrusted (§6.1).

- Set `GIT_ALLOW_PROTOCOL=file:https:ssh:git` for every git command. Other transports (notably `ext::`) are refused. `file://` stays allowed: local forks are a legitimate use and the user can read those files anyway. The `REFS_GIT_ALLOW_PROTOCOL` environment variable overrides the list.
- Put `--` (or `--end-of-options` where git supports it) before any user-supplied `url` or `ref`.
- Cache repos are created by refs and carry no hooks.

## 8. Block content

### 8.1 Header line

One line per repo:

```
[<id> @ <ref> <sha7>] <description>. Packages: <p>, <p>. Start: <path>, <path>.
```

- Omit empty fields (no description, no `Packages:`, no `Start:`). When the ref is a SHA, show `@ <sha7>` once.
- `<ref>` is the ref as configured, except for `HEAD`: if the lock has a `branch` (§10.2), show that (`@ main abc1234`); otherwise show `HEAD`. A lock written before `branch` existed renders `HEAD` until the next `lock` re-resolves it.
- Only active repos render (§6.4). Repos render in config order within their group; groups in order of first definition; ungrouped repos last under `### Ungrouped`. A group with no active repos renders no heading.
- Under each group heading, the group's description (if any), then a blank line, then the header lines, all sit in one ```` ```text ```` fence, so Markdown formatters leave them alone. Only the `###` heading and the fixed preamble are plain Markdown.
- The fence exists because of a verified problem in the pilot: oxfmt rewrote `|` lines as a table. A blank line follows `<!-- BEGIN:refs -->`, which oxfmt requires.
- The block must be stable under common Markdown formatters (oxfmt, prettier, dprint), or `sync --check` fights `fmt --check`.

### 8.2 Determinism

Output depends only on (config, lock, generator version). Same inputs → byte-identical block.

The fixed prose changes between `refs` releases, so a teammate or CI on a different version would fail `sync --check` forever. `sync` records the version that last wrote the block as `generator` in `refs.lock` (updated only when it rewrites a block). If `--check` finds the block differs and the running version differs from `generator`, it exits 3 with `refs::sync::generator_mismatch` ("block was generated by refs X, this is Y; run `refs sync`") instead of a plain "out of date". `sync` run twice is a no-op. `packages`, `start` and descriptions change only the rendered block, not the lock.

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

Source and docs for some dependencies are checked out read-only in `<references_dir>/`, pinned in `refs.lock`. They can be newer than your training data. Prefer retrieval-led reasoning over pre-training-led reasoning for these libraries: read the relevant files before writing code that uses them. Look up an imported API in a repo whose `Packages:` lists the package it's imported from (e.g. imports from `<first package>` → `<references_dir>/<its repo id>`); APIs with the same name can behave differently in different packages.

`<references_dir>/` is excluded from default search. To search it, pass the path explicitly, e.g. `<references_dir>/<first repo id>`. Don't edit anything in `<references_dir>/`; it is regenerated by `refs sync`. If `<references_dir>/` is missing, tell the user to run `refs sync`.

### SolidJS 2.0

```text
Solid 2.0 release candidates, router and docs. Newer than your training data; many APIs differ from Solid 1.x.

[solid @ next ee49b3e] Solid 2.0 core packages and 2.0 design docs. Packages: solid-js, @solidjs/web, @solidjs/signals. Start: documentation/solid-2.0/MIGRATION.md.
[solid-router @ next 4658036] Solid Router 2.0 source. Packages: @solidjs/router.
```

<!-- END:refs -->
````

- `<first package>` is the first package of the first repo that has one. If no repo lists packages, omit the lookup sentence.
- `<references_dir>` is `settings.references_dir` (default `.references`), substituted into the fixed prose. It comes from the committed config, so determinism (§8.2) holds.
- Fixed prose is short; group descriptions come from config.

---

## 10. CLI

Semantics follow `uv`: config is intent, the lock is resolution, `sync` makes the disk match the lock. Nothing updates behind the user's back.

Global flags: `--project <dir>`, `-q/--quiet`, `-v/--verbose`, `--no-color`.
Exit codes: `0` ok, `1` error, `2` usage error, `3` `--check` found something out of date.

| Command | Behaviour |
|---|---|
| `refs init [--here]` | Create `refs.toml` (commented template) if absent, add the empty managed block to `agents_files`, add the exclude rule. Print the linter/formatter reminder and the CLAUDE.md note. Idempotent. Warns and stops if a parent directory already has a `refs.toml`, unless `--here` (§5). |
| `refs add <url> [--id <id>] [--group <g>] [--ref <r>] [--paths <p>…] [--packages <p>…] [--start <p>…] [--description <d>] [--no-sync]` | Append a repo via `toml_edit`. Id defaults to the URL's last path segment. Creates the group stub if missing. Unless `--no-sync`, runs `lock` for that repo and then `sync`. |
| `refs remove <id> [--no-sync]` | Remove the repo entry, leaving comments and other entries alone. Remove an empty group only if it has no description. Then `sync` unless `--no-sync`. |
| `refs enable <id> [--group] [--no-sync]`, `refs disable <id> [--group] [--no-sync]` | Set or clear `enabled` on a repo, or on a group with `--group` (§6.4). Then `sync` unless `--no-sync`. |
| `refs list` | Repos grouped: id, url, requested ref, locked SHA (short), checkout state (ok / missing / wrong SHA / not locked / disabled). |
| `refs lock [--upgrade [<id>…]]` | Resolve refs → SHAs and write `refs.lock`. Without `--upgrade`, an already-locked repo keeps its SHA (and `branch`) while its `url`, `ref` and `source` are unchanged; a changed `paths` only refreshes the lock entry's `paths`, so editing `paths` never moves a floating ref. A repo that was disabled and is enabled again is re-resolved (§6.4). With `--upgrade`, re-resolve floating refs (all, or the given ids), refreshing `branch` for `HEAD` refs. No checkouts touched. Also checks `start` paths (§7.2). |
| `refs sync [--check] [--locked] [--offline] [--force]` | See below. `--locked`: error instead of locking when the lock is missing or stale. `--check`: use the existing lock only; never resolve refs, fetch, or write. Exit 3 if the lock, checkouts or blocks are out of date (CI, pre-commit). Floating-ref movement upstream is detected only by `lock --upgrade`. `--offline`: never contact the network; requires a current lock and all objects needed to materialise the desired checkouts, including the blobs for the sparse paths (§7.3 step 1b), already in the cache. Missing required objects are an error. Combined with `--check`, inspect existing state without fetching or materialising; out-of-date state exits 3. `--force`: discard local changes in checkouts (§7.3). |
| `refs doctor` | Diagnose without changing anything (§10.1). |

`sync` steps:

0. Compute the active set from the config (§6.4). Steps 1–5 operate on active repos; step 6 applies to the project.
1. Determine whether the lock is current. A normal sync locks if it is missing or stale, unless `--locked`, which errors. With `--offline`, a missing or stale lock is an error. With `--check`, a missing or stale lock is out of date (exit 3); do not resolve refs or write the lock. In check mode, this out-of-date result takes precedence over `--locked`'s error behavior.
2. Ensure required objects (commits, trees and the blobs the checkouts need, §7.3 steps 1 and 1b) are in the bare blobless cache, fetching them unless `--offline` (which errors if they are missing). In check mode, inspect current state without fetching.
3. Update the sparse worktrees and prune removed repos. In check mode, inspect them without updating or pruning.
4. Check `start` paths.
5. Render the block; write it only if it changed. In check mode, compare without writing.
6. Ensure the exclude rule. In check mode, report a missing rule as out of date (exit 3) without writing it.

### 10.1 `doctor` checks

- git present and at least 2.36.0 (§7.5).
- Project config parse and validate: unknown keys, bad ids, dangling group refs.
- `start` path missing, or not in the checkout (§4): error.
- A package listed under more than one active repo: info.
- Number of disabled repos and groups: info.
- Lock present and its `config_hash` matches.
- Each checkout exists, is a worktree of the right cache repo, is at the locked SHA, and has the right sparse patterns. A dirty checkout: warn.
- Orphan directories in `references_dir` not matching any repo.
- Cache worktree registrations whose directory is missing: info (the next `sync` prunes them and recreates this project's; §7.3).
- Exclude rule present.
- Server ignored the blob filter (cache clone is a full clone; detection method to be settled in milestone 4): info (§7.4).
- Managed markers present and well-formed in each `agents_files` entry.
- `CLAUDE.md` vs `AGENTS.md`:
  - `CLAUDE.md` exists, `AGENTS.md` holds the block, and `CLAUDE.md` neither imports (`@AGENTS.md`) nor symlinks to it: warn (Claude Code loads only `CLAUDE.md`).
  - `CLAUDE.md` in `agents_files` and also importing/symlinking `AGENTS.md`: warn (double load).
- Reminder that linters/formatters/type checkers may scan `references_dir`.
- Submodules or LFS pointers detected in a checkout: info.

Each finding is `ok` / `warn` / `error` with a stable code and a one-line fix. These are miette diagnostics (ADR 0002): `warn` is `Severity::Warning`, `error` is the default severity, the fix is `help`. Exit 1 if any error.

### 10.2 Lock file format

TOML, sorted by repo id, stable formatting:

```toml
# Generated by refs. Do not edit.
version = 1
generator = "refs 0.1.0"   # version that last wrote the block (§8.2)
config_hash = "sha256:…"   # hash of the normalised active repo set: (id, source, url, ref, paths)

[[repo]]
id = "solid"
source = "git"              # discriminator; other kinds add their own fields later
url = "https://github.com/solidjs/solid"
ref = "next"
sha = "1a2b3c4d5e6f…"      # full 40 hex; always a commit (peeled), never a tag object (§7.2)
branch = "main"             # only when ref = "HEAD": the remote's default branch at resolve time (§7.2); display only
paths = ["packages", "documentation"]
```

The entry is a tagged union on `source`. `git` is the only kind in the MVP and keeps git-specific field names (`url`, `ref`, `sha`). Adding a kind is additive and needs no `version` bump. The same `source` key is accepted, optional and defaulting to `"git"`, on `[repos.<id>]` in `refs.toml`. Code outside the source module treats the resolved identity as an opaque pin.

`config_hash` covers the active repo set and the fields that affect resolution and checkout: each repo's `id`, `source`, `url`, `ref` and `paths`. A change to any of those, or to the active set (including enable/disable), makes the lock stale. Stale does not mean re-resolve: `lock` re-resolves only when `url`, `ref` or `source` changed or the repo is newly active; a `paths`-only change updates the entry in place (§10). A floating ref whose upstream branch was renamed is noticed only by `lock --upgrade`, like any floating-ref movement. Group membership and display fields (`description`, `name`, `packages`, `start` and the lock's `branch`) don't stale the lock; they only affect the block on the next `sync`. `--locked` errors on a missing lock or a config-hash mismatch. `sync --check` reports either as out of date with exit code 3.

---

## 11. Implementation notes

- **Crates (suggested):** `clap` (derive), `serde` + `toml` with the `preserve_order` feature (read; keeps config order, §6.1), `indexmap` with `serde` (ordered `groups`/`repos` maps), `toml_edit` (write), `thiserror` + `miette` (diagnostics, ADR 0002; no `anyhow`), `fd-lock`, `sha2`, `etcetera` or `directories` (XDG paths), `similar` for `--check` diffs in verbose mode.
- **Git:** shell out via `std::process::Command`; no libgit2 or gix in the MVP. One module with a thin, testable wrapper. Set `GIT_TERMINAL_PROMPT=0` for non-interactive commands so CI fails fast.
- **Diagnostics:** library errors are `thiserror` enums deriving `miette::Diagnostic` with codes named `refs::<area>::<name>` (e.g. `refs::config::bad_id`; areas `config`, `lock`, `git`, `sync`, `block`, `doctor`). Only `refs.toml` validation errors carry spans, and all of them are collected and reported together (`#[related]` on a wrapper error, one `NamedSource` per child). Config checks for untrusted input get codes too (`refs::config::bad_url` for option-like values and passwords in URLs, `refs::config::bad_ref`, `refs::config::path_not_dir`). Git failures the spec treats specially get their own code (ref not found, ambiguous ref, path missing at SHA, `start` not in the checkout, foreign directory in `.references/`, dirty checkout, generator mismatch under `--check`, git too old, server refuses fetch-by-SHA); everything else is `refs::git::failed` with the trimmed stderr. Don't parse stderr to detect auth failures. Only the binary enables miette's `fancy` feature.
- **Atomic writes** for `refs.lock` and AGENTS.md: temp file in the same directory, then rename.
- **Paths:** `camino` or careful `Path` handling; forward slashes in rendered output.
- **Windows:** not a target for the MVP; avoid gratuitous Unix-only assumptions.

### 11.1 Milestones

1. **Git layer:** cache, `lock`, sparse worktrees, exclude rule. Spikes are done (§7). Acceptance: the git lifecycle test (§12) passes.
2. **Block renderer plus `sync`**, with golden tests, including formatter stability.
3. **`init`, `add`, `remove`, `list`** via `toml_edit`.
4. **`doctor`.**

---

## 12. Tests

- **Resolver fixture:** recorded `ls-remote` output containing an annotated tag (with `^{}` line), a lightweight tag, and a branch/tag name collision; assert the resolved SHAs and the ambiguity error as data, no git. Also an annotated tag whose `^{}` line is absent → peel-locally path.
- **Golden block:** fixture repos built in tests as local `file://` bare repos; snapshot the rendered block (e.g. `insta`). Cover missing fields, SHA refs, ungrouped repos, no packages anywhere, a `HEAD` ref with and without a `branch` in the lock, a group description (inside the fence), and a non-default `references_dir` in the prose.
- **Formatter stability:** run oxfmt / prettier / dprint over a file containing the block, including `proseWrap=always` and group names at the edge of the allowed character set; output must be unchanged.
- **Diagnostics:** assert on codes, not message strings. `Diagnostic::code()` returns `Option<Box<dyn Display>>`, so the assertion is `d.code().unwrap().to_string()`.
- **Pin reuse:** editing `paths` on a repo with a floating ref keeps its locked SHA and updates the entry's `paths`; changing `ref` or `url` re-resolves; disable then enable re-resolves.
- **Untrusted input:** a `url` or `ref` starting with `-`, a URL with a password, and a `paths` entry that is a file are rejected with their codes; `ext::` URLs are refused by the protocol allowlist; `--` precedes user-supplied values.
- **Idempotence:** `sync` twice → second run writes nothing (`--check` exits 0).
- **Round-trips:** `add` then `remove` leaves `refs.toml` byte-identical, including comments.
- **Git lifecycle:** add → lock → sync → change ref → lock --upgrade → sync → remove → sync; moved project dir → sync repairs; a checkout deleted by hand (stale registration) → sync recreates it; two projects sharing one cache repo with different sparse paths; concurrent syncs.
- **Blobless and offline (`git daemon` fixture, `uploadpack.allowFilter=true`):** the cache clone is partial; the §7.3 step 1b prefetch brings exactly the sparse blobs; `sync --offline` succeeds once they are present and errors, naming the missing objects, when they are not. One opt-in test repeats the blobless check against a real remote.
- **Dirty checkout:** an untracked file blocks `sync` with `refs::sync::dirty_checkout`; `--force` clears it.
- **Reproducibility:** the committed project config and lock fully determine the generated block; no machine-local config is read.
- **Block safety:** content outside markers untouched; malformed markers refuse to write.
- **`start` validation:** missing path and not-in-checkout errors; a root-level file and an ancestor-directory file are accepted (cone mode); rendered strings with newlines, ```` ``` ```` or marker text are rejected.
- **Enabled flag:** disabling a repo drops its lock entry, checkout and Entry; disabling a group cascades; a group with no active repos has no heading; `disable` then `enable` round-trips `refs.toml` byte-identical; disabled entries still fail validation on bad ids and dangling groups.

---

## 13. Evaluation plan (after the MVP works)

Out of scope for building the CLI; recorded so the evals can resume.

- Narrow tasks that force the target behaviour (e.g. "add save/delete using `@solidjs/router` actions"), block vs no block, at least 2 runs each. Whole-app runs cost about 2–3M input tokens each and hit the target in about half of them.
- Judge from transcripts, not self-reports; check `message.model`; pin the model and use an identical kickoff message with no mid-run steering.
- Variables: lookup-sentence wording; header-only vs an optional inline tree; the missing-checkout line.

---

## 14. Open questions

1. Final wording of the block, to be settled by evals.

Resolved: the lock records the default branch for `HEAD` refs and the block shows it (§8.1, §10.2); a `paths` edit never moves a pin (§10); blobs are prefetched so `--offline` is checkable (§7.3); group text renders inside the fence (§8.1); `url`/`ref` are validated and git runs with a protocol allowlist (§6.1, §7.7). Minimum git is 2.36.0 (§7.5). A package listed under several repos is rendered as written (each repo's line lists it) and `doctor` reports it as info. A repo has exactly one group; there is no multi-group membership (a group is only a block heading plus a "when to look here" description, and lookup goes through `Packages:`).
