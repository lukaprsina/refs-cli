# `refs` — local reference repos for coding agents

**Status:** MVP spec. Vocabulary: `GLOSSARY.md`. Decisions: `docs/adr/`.
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
- CLI: `init`, `add`, `remove`, `enable`, `disable`, `list`, `lock`, `sync`, `update`.

### 3.2 Deferred (rough priority order)

- **Next, not ranked:** `refs doctor` (§10.1) and everything that mentions it. It is designed to reuse the `Observed` state and the `plan` that `sync --check` already computes, so it adds no new inspection code. Until it exists, `lock`/`sync` check the git version themselves (§7.5) and `init` prints the reminders.
- Also: peeling an unpeeled annotated tag locally (fetch the tag object, `rev-parse <sha>^{commit}`); listing every missing object for `sync --offline` (`GIT_NO_LAZY_FETCH=1 git cat-file --batch-check`; never `rev-list --missing=print`); the `REFS_GIT_ALLOW_PROTOCOL` override; `sync --locked` and verbose `--check` diffs (`-v`, `similar`). `--check` already reports a stale lock for CI.

Ranked:

1. `add` infers `packages` from `package.json` `name`s under `paths`. This becomes the default way `packages` gets filled; `--packages` (and the other `add` metadata flags) only override it.
2. An optional inline tree as an add-on to the header lines (Vercel favoured one; revisit if evals show a gap).
3. Per-file hints (titles, export names), if evals show a need. Markdown/MDX via a real parser (`pulldown-cmark` or `markdown`), never regex scraping.
4. Linter/formatter exclusion: generating ignore entries for common tools. In the MVP this is the user's job; `init` says so.
5. Ref resolution from package lockfiles (`package.json`/`pnpm-lock.yaml` version → git tag).
6. `--json` output.

**CLI alignment with uv (ADR 0005), after the architecture polish and before rank 1.** One batch, no new features before it:

- `--upgrade` takes no values; `--upgrade-package <id>` (repeatable) names repos.
- `lock --check`; `sync --locked` and `--frozen`.
- `--offline` global rather than on `sync` only; `-v`/`--color`, short flags where uv has them.
- `remove`, `enable` and `disable` take several ids: one edit, one lock, one sync.
- `add` input shorthand: an explicit `gh:owner/repo` (alias `github:`) prefix, never a bare `owner/repo`. Input sugar only: `refs.toml` stores the expanded URL. Further hosts later (`gl:`).
- `refs cache clean`/`prune` for the global cache (formerly `gc`/`clean`, rank 6). Required before any release.

**Interactive `add` (ADR 0008), an exception to "no features before it".** It goes before the batch and before rank 1. On a terminal, `add` asks for what it was not given (a bare `refs add` asks for the URL too); `--no-input` (global) turns it off, and without a terminal nothing is asked. When rank 1 lands, its inferred `packages` and `start` become the prompt defaults.

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
- **Start:** optional per repo; repo-relative files worth reading first (e.g. a migration guide). Each must be in the checkout: inside a `paths` entry, or (cone mode always includes these, verified) a file directly in the repo root. Files in ancestor directories of a `paths` entry are also checked out by cone mode, but are not accepted as `start` values.
- **Active:** a repo is active when it and its group are both enabled (`enabled`, default `true`). Only active repos are locked, checked out and rendered. Everything downstream of config parsing sees only the active set.
- **Ref:** branch, tag, or full commit SHA. Omitted → `"HEAD"` (the remote's default branch).
- **Lock:** the resolved commit SHA for each repo, committed to the project.
- **Checkout:** a sparse, detached-HEAD `git worktree` at `.references/<repo>/`.
- **Managed block:** the generated section in `AGENTS.md` between markers.

---

## 5. Files and locations

| File | Location | Committed? | Written by |
|---|---|---|---|
| Project config | `<project>/refs.toml` | yes | user, `init`, `add`, `remove` |
| Lock | `<project>/refs.lock` | yes | `lock`, `sync` |
| Global cache | `$XDG_CACHE_HOME/refs` (default `%LOCALAPPDATA%\refs` on Windows, else `~/.cache/refs`) | n/a | `lock`, `sync` |
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
agents_files = ["AGENTS.md"]       # project config only; files that receive the managed block

[groups.<id>]
name = "Human name"                # optional, the id when absent; letters, digits, spaces and . , : ( ) / + & - only (rendered as a heading)
description = "When to consult this group"   # optional, strongly recommended
enabled = true                     # optional, default true; false disables the group and all its repos

[repos.<id>]
url = "https://github.com/owner/repo"   # required; https://, ssh://, git://, file:// or scp-style user@host:path; no http://; no password, no leading "-"
group = "<group id>"                    # optional; ungrouped repos render under "Ungrouped"
ref = "main"                            # optional; branch, tag, or 40-char SHA; no leading "-"
description = "One line on what this is" # optional, recommended
paths = ["packages", "documentation"]   # optional; directories; sparse checkout (cone mode); absent = whole repo
packages = ["solid-js", "@solidjs/web"] # optional; rendered as "Packages:"
start = ["documentation/solid-2.0/MIGRATION.md"]  # optional; rendered as "Start:"
enabled = true                          # optional, default true; false = treated as absent (§6.4)
```

`<id>` for repos is the directory name under `.references/`; validate `[a-z0-9][a-z0-9._-]*`, not `.` or `..`, unique within the config.

Validation: `paths` entries are relative and `/`-separated (no `\` or `:`), no `..`; when `paths` is present, every `start` is inside a `paths` entry or a direct child of the repo root (cone mode checks root files out too). With `paths = ["docs/guide"]`, `README.md` and `docs/guide/a.md` are valid `start` values; `docs/README.md`, `src/x.rs` and `docs/guide/../x.md` are not. A direct child of the root must be a file: `sync` and `lock` reject a root directory with `refs::git::start_not_file`, since cone mode does not check it out.

`settings.references_dir` and each `settings.agents_files` entry are project-relative output paths. Reject absolute paths, paths containing `..`, empty paths, and paths that resolve to the project root. These lexical checks run at config time. Resolving symlinks (the destination, or its nearest existing ancestor, must stay inside the canonical project root; broken symlinks are rejected) and checking that an existing `references_dir` is a directory and each `agents_files` target a regular file belong to `project` and run at `sync` time; they are not race-resistant against a symlink changed between validation and writing.

Every string that is rendered into the block (`name`, `description` on groups and repos, `packages` and `start` entries) is a single line: no control characters (including newlines), no ```` ``` ````, and neither `BEGIN:refs` nor `END:refs`. Otherwise a config value could end the fence or the markers early.

Free text is rendered inside the `text` fence (§8.1), where formatters leave it alone. The one exception is a group `name`, which is the `###` heading: allow only Unicode letters and digits, spaces and `. , : ( ) / + & -`, not starting or ending with a space, so a formatter has nothing to rewrite.

**`url` and `ref` come from a file that may belong to an untrusted repository**, and `refs sync` hands them to git:

- Reject a `url` whose scheme is not `https`, `ssh`, `git` or `file`, or that is not scp-style `user@host:path` (a host starting with `-` is rejected too; so `http://`, `ext::` and `fd::` fail at config time, with `refs::config::bad_url`; `GIT_ALLOW_PROTOCOL`, §7.7, stays as the second layer).
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

`add`/`remove`/`enable`/`disable` must preserve comments, ordering and formatting. Use `toml_edit` for writes, except where ADR 0007 says otherwise (`remove`, `enable` and `disable` cut by text); `serde` + `toml` for reads.

### 6.4 Disabled repos and groups

A disabled repo behaves as if it were commented out of the config, but stays visible to tooling. Use it to keep a reminder of where code came from (e.g. a proof-of-concept repo an agent already copied from) and switch it back on later.

- **Active set:** repo active ⇔ `repo.enabled` and its group's `enabled` are both true. Disabling a group cascades to every repo in it. An ungrouped repo depends only on its own flag.
- **Lock:** a disabled repo has no lock entry. Disabling then running `lock`/`sync` drops its entry.
- **Checkout:** `sync` removes the checkout of a disabled repo like any repo no longer in the config (§7.3).
- **Block:** a disabled repo has no Entry. A group with no active repos renders no heading (this holds for enabled groups that simply have no repos too).
- **Re-enabling** re-resolves the ref, exactly as uncommenting would. A floating ref may land on a newer commit than before; the old pin isn't kept.
- **Validation still applies** to disabled entries (id, dangling group, `paths`, `start` syntax), but nothing is resolved or fetched for them. `list` shows them marked `disabled`; the later `doctor` reports how many are disabled (info).
- Disabling and groups stay despite having no `uv` counterpart (ADR 0005): Repos and Groups are not colocated in the TOML, so a switch on either is not the same as editing a line.
- `refs disable <id>` / `refs enable <id>` set `enabled` on a repo; `--group` targets a group id instead. Both edit via `toml_edit`, then lock and sync like `add` (§10). `enable` removes the key rather than writing `enabled = true`.

---

## 7. Storage and git mechanics

Shell out to the system `git` binary: it handles auth (credential helpers, SSH agents, private repos) for free. Spikes ran on git 2.55.0. **Minimum git is 2.36.0** (§7.5); `GitSource` checks the version before its first git command.

### 7.1 Global cache

- One **bare, blobless** clone per normalised URL: `<cache_root>/git/<sha256(normalised_url)[..16]>/`, where `<cache_root>` is `$XDG_CACHE_HOME/refs` (default `%LOCALAPPDATA%\refs` on Windows, else `~/.cache/refs`). Create it with `git clone --bare --filter=blob:none <url>`. Store the original URL in a small metadata file beside it.
- URL normalisation: strip trailing `/` and `.git`, lowercase the host. Do not rewrite protocols: `https://github.com/o/r` and `git@github.com:o/r` get separate cache clones. Unifying them would mean guessing each host's URL mapping.
- Guard every mutation of a cache repo with an exclusive file lock (e.g. `fd-lock`).
- **Blobless is not shallow.** The clone and each fetch still bring every commit and tree reachable from the requested object. That is small for the repos this tool targets and large for something like `torvalds/linux` (hundreds of MB). Accepted for the MVP; a shallow or `tree:0` mode is a later option.
- **A bare clone has no fetch refspec** (verified): a plain `git fetch origin` fetches objects but updates no refs, only `FETCH_HEAD`. So never rely on refs in the cache. Resolve refs with `git ls-remote`; fetch by SHA (`git fetch origin <sha>`), falling back to fetching the ref if the server refuses SHA wants.
- `--filter=blob:none` is verified against a real remote only (GitHub, git 2.55.0): the clone downloads trees and commits only, `git fetch origin <sha>` works, and a sparse `checkout` downloads just the blobs it needs (10 → 4 missing blobs on a 10-blob test repo). A local `file://` server honours the filter only when the source repo sets `uploadpack.allowFilter=true` (verified on git 2.55.0: partial clone, 11 missing blobs); without it git prints `warning: filtering not recognized by server, ignoring` and silently makes a full clone. Fixtures therefore set `uploadpack.allowFilter=true` on their bare source repos and assert the clone is partial (`remote.origin.partialclonefilter`), so a silently full clone fails the test. A `git daemon` fixture behaves the same (verified) and is only needed if `file://` misbehaves on the CI git version. Keep one opt-in test against a real remote (GitHub) for server differences (§12).

### 7.2 Resolution (`lock`)

- **Branch or tag:** one round trip with explicit patterns: `git ls-remote <url> refs/heads/<ref> refs/tags/<ref> 'refs/tags/<ref>^{}'`. The `^{}` pattern matches literally and returns the peeled commit line for an annotated tag (nothing extra for a lightweight one). Prefer tags over branches if both match; error if still ambiguous. Branch/tag name collisions are common in the repos people reference, so this is a real path.
- **`HEAD`:** `git ls-remote --symref <url> HEAD` returns the target branch and its SHA. The lock records the branch name (`branch`, §10.2) so the block can show `@ main` instead of `@ HEAD` (§8.1). If the server sends no `ref:` line (detached remote HEAD), `branch` is omitted.
- **Prefer targeted patterns over a bare `ls-remote`**: a bare listing is ~2000 refs on typical targets (`solidjs/solid` 1967, `torvalds/linux` 3828, ~0.8s each).
- **The lock stores the commit SHA, never a tag object SHA.** For an annotated tag, plain `ls-remote <url> <ref>` returns the tag object and no `^{}` line, and `worktree add` and `ls-tree` peel silently, so a tag SHA in the lock would check out a different commit than the one locked. `sync --check` would then fail forever and §8.1 would render an unlookable SHA, with no error anywhere. Resolve to the commit upstream of the lock.
- **No fallback in the MVP:** if the server omits `^{}` for an annotated tag, `lock` fails with `refs::git::unpeeled_tag` (never store the tag object SHA). Peeling locally is deferred (§3.2).
- A 40-hex ref is taken as-is and verified on fetch. Abbreviated SHAs are rejected. Only 40-hex (SHA-1) object ids are supported; SHA-256 repositories (64 hex) are out of scope for the MVP.
- For each configured path, verify it exists at the resolved SHA and is a tree (`git ls-tree`). A missing path, or a path that is a file, is an error naming repo, path and SHA.
- For each `start` path, verify it exists at the SHA and will be in the checkout (inside `paths`, or a direct child of the root; §4). A missing one is an error (`refs::git::start_missing`). With `paths` set, a slash-free `start` must be a file: cone mode checks out the root's files, not its directories (`refs::git::start_not_file`).

### 7.3 Checkouts (`sync`)

For each locked repo:

1. Ensure the cache clone has the SHA.
1b. Prefetch the blobs the checkout will need, under the cache lock, so all network access happens in steps 1 and 1b and `checkout` never fetches lazily. The needed set is the blobs directly in the repo root (`git ls-tree <sha>`, cone mode checks them out), the blobs directly in every ancestor directory of a `paths` entry (`git ls-tree <sha> -- docs/ docs/guide/`; cone mode checks those out too), and every blob under the `paths` (`git ls-tree -r <sha> -- <paths>`), or the whole tree when `paths` is absent, deduplicated by OID and minus those the cache already has (`git cat-file --batch-check`). Fetch them in one round trip with the options git's own lazy fetch uses: `git -c fetch.negotiationAlgorithm=noop fetch origin --no-tags --no-write-fetch-head --recurse-submodules=no --filter=blob:none --stdin`, OIDs on stdin (verified against GitHub, git 2.55.0; without the `noop` negotiation GitHub answers a want for a blob with `did not send all necessary objects`, which a local `file://` server does not reproduce). Prefetching only the `paths` makes a no-lazy checkout fail with `could not fetch <oid> from promisor remote`, and so does leaving out the ancestor files. `--offline` is minimal: it skips steps 1 and 1b, and a missing object makes `checkout` fail, which `GitSource` reports for that Repo naming the object (and it removes the directory it just created). `cat-file` lazy-fetches in a partial clone unless `GIT_NO_LAZY_FETCH=1` is set. Run `checkout` with `GIT_NO_LAZY_FETCH=1` as a backstop.
2. If `.references/<repo>/` doesn't exist: run `git -C <cache> worktree prune` first (a deleted checkout, e.g. after `git clean -fdx`, leaves a stale registration that makes `worktree add` fail), then `git -C <cache> worktree add --no-checkout --detach <project>/<references_dir>/<repo> <sha>`, then `git -C <worktree> sparse-checkout set --cone <paths>` (skip when `paths` is absent), then `git -C <worktree> checkout --detach <sha>`.
3. If it exists at another SHA, or with other `paths`: after steps 1 and 1b (so a failure there leaves the old Checkout alone), remove the worktree (`git worktree remove --force`, then `prune`) and create it again from step 2, all under the cache lock. Changing the sparse patterns and the commit in place would need blobs that 1b did not fetch (the new paths at the old commit, or the old paths at the new one); a fresh Checkout reads exactly the prefetched set. A Checkout of another remote (the URL changed) is removed from its own Cache entry first. What a Checkout was made from (its Pin and `paths` as written) is kept in `refs-checkout.toml` in the worktree's admin directory in the cache, so `inspect` reports the Pin with its ref; a record that disagrees with the checked-out commit counts as out of date.
4. If `.references/<repo>/` exists but isn't a worktree of the expected cache repo, stop with `refs::sync::foreign_dir`; never delete a directory refs didn't create. The exception is a **dangling checkout**: its `.git` file points at a gitdir under the cache root that no longer exists (the cache was wiped, e.g. `rm -rf ~/.cache/refs` or a cache cleaner). Checkouts are generated, read-only copies and their history is gone, so `sync` removes the directory and recreates it from step 2, printing `refs::sync::recreated` (info). A directory with no `.git`, or whose `.git` points outside the cache root, is still foreign.

**Sparse config is per-worktree** (verified): the first `sparse-checkout set` enables `extensions.worktreeConfig` in the cache and writes patterns to `worktrees/<name>/config.worktree`. Two worktrees of one cache hold different patterns; nothing leaks across projects. The cache stays bare (its top-level `core.bare = true` is untouched).

Also:

- Remove worktrees for repos no longer active (removed from the config or disabled, §6.4): `git worktree remove --force` (generated, read-only copies). **Unless the checkout is dirty**: if `git status --porcelain` in it shows any modified or untracked file, `sync` refuses to remove it or move it to another SHA and reports `refs::sync::dirty_checkout` naming the files, so the "never delete a directory refs didn't create" rule of step 4 also holds for edits an agent made despite the block. Untracked files count (a `node_modules` created by running a tool inside a checkout blocks `sync` too); the diagnostic says so and names `sync --force`, which discards them.
- Run `git worktree prune` on each touched cache repo. Prune forgets registrations whose directory is missing, including projects on a currently unmounted drive; their checkouts are recreated by the next `sync`. Accepted; the later `doctor` reports such registrations as info (§10.1).
- Moving a project directory breaks worktree links; `GitSource` repairs the links with `git worktree repair` (silently) or, if that cannot work, `inspect` reports the checkout as dangling and `sync` recreates it.
- **Recreating is announced.** When `sync` recreates a dangling checkout it prints an info-level diagnostic (`refs::sync::recreated`). `-q` silences it. Repairing a moved project and pruning stale registrations (`git worktree repair`, `git worktree prune`) happen silently inside `GitSource`; announcing them (`refs::sync::repaired`, `refs::sync::pruned`) is deferred.
- Ignore submodules and Git LFS content in the MVP; the later `doctor` mentions them if detected.

### 7.4 Server prerequisites

`--filter=blob:none` needs `uploadpack.allowFilter` on the server; fetch-by-SHA needs protocol v2 (default since git 2.26) or `uploadpack.allowReachableSHA1InWant`. GitHub and GitLab satisfy both; a self-hosted `git-http-backend` may not. When the server ignores the filter, the later `doctor` reports it as info (§10.1).

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

`GitSource` runs `git --version` once, before its first git command, and fails with `refs::git::too_old` below 2.36.0. Until `doctor` exists this is the only version check.

### 7.6 Exclusion

- `sync` and `init` ensure `/<references_dir>/` is a line in `<git-dir>/info/exclude` (the common git dir for linked worktrees of the project). Found with `git rev-parse --git-common-dir`, so a linked worktree writes to the main repository's file. A project below the repository top gets the rule anchored there, `/<path from the top>/<references_dir>/`. Per-clone, which is fine since every clone runs `sync`.
- If the project isn't in a git repo, `plan` emits an info note and continues; it is not drift.
- Consequence we rely on: default searches (ripgrep) skip `.references/`, so a grep for `useEffect` finds only the user's code. When a search path is passed explicitly (e.g. `.references/solid`), ripgrep searches it anyway. Hidden entries inside a checkout (e.g. `.github/`) stay skipped.
- Linters, formatters and type checkers are **the user's responsibility** in the MVP. `init` prints a reminder (and the later `doctor` repeats it).

---

### 7.7 Running git on untrusted input

`refs.toml` may come from a repository the user just cloned, so `url` and `ref` are untrusted (§6.1).

- Set `GIT_ALLOW_PROTOCOL=file:https:ssh:git` for every git command. Other transports (notably `ext::`) are refused. `file://` stays allowed: local forks are a legitimate use and the user can read those files anyway.
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
- Under each group heading, the group's description (if any), then a blank line, then the header lines, all sit in one ```` ```text ```` fence, so Markdown formatters leave them alone. The fixed preamble sits in a `text` fence too, because with `proseWrap=always` the formatters re-wrap plain paragraphs, each at its own width (prettier and dprint at 80, oxfmt at 100), so no hard wrapping is stable under all three. Only the markers and the `##` and `###` headings are plain Markdown.
- The fence exists because of a verified problem in the pilot: oxfmt rewrote `|` lines as a table. A blank line follows `<!-- BEGIN:refs -->`, which oxfmt requires.
- The block must be stable under common Markdown formatters (oxfmt, prettier, dprint), or `sync --check` fights `fmt --check`.

### 8.2 Determinism

Output depends only on (config, lock, `refs` version). Same inputs → byte-identical block.

The fixed prose changes between `refs` releases, so a teammate or CI on a different version would fail `sync --check` forever. `--check` reports that as plain "out of date" (exit 3). A dedicated `generator_mismatch` diagnostic, with a `generator` field in the lock recording the version that wrote the block, is deferred. `sync` run twice is a no-op. `packages`, `start` and descriptions change only the rendered block, not the lock.

---

## 9. The managed block

### 9.1 Markers and placement

```
<!-- BEGIN:refs -->
…generated…
<!-- END:refs -->
```

- Only content between the markers is ever touched. If the markers are missing, `sync` appends the block at the end of the file (creating it if needed). Unbalanced, nested or duplicated markers: error, don't write.
- Written to each file in `settings.agents_files` (default `AGENTS.md`). Write only if the content changed.
- Claude Code loads `AGENTS.md` only when `CLAUDE.md` doesn't exist. the later `doctor` checks the relationship (§10.1) but `sync` never creates or modifies `CLAUDE.md` unless it's in `agents_files`.
- If `CLAUDE.md` is in `agents_files` **and** also imports `AGENTS.md` (`@AGENTS.md`) or is a symlink to it, the block loads twice: the later `doctor` flags it.

### 9.2 Content template

The exact wording is an eval variable and can change later. Keep the instruction count tiny and the tone plain: no "MUST", no capitals.

````markdown
<!-- BEGIN:refs -->

## Reference repos

```text
Source and docs for some dependencies are checked out read-only in `<references_dir>/`, pinned in `refs.lock`. They can be newer than your training data. Prefer retrieval-led reasoning over pre-training-led reasoning for these libraries: read the relevant files before writing code that uses them. Look up an imported API in a repo whose `Packages:` lists the package it's imported from (e.g. imports from `<first package>` → `<references_dir>/<its repo id>`); APIs with the same name can behave differently in different packages.

`<references_dir>/` is excluded from default search. To search it, pass the path explicitly, e.g. `<references_dir>/<first repo id>`. Don't edit anything in `<references_dir>/`; it is regenerated by `refs sync`. If `<references_dir>/` is missing, tell the user to run `refs sync`.
```

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

Semantics follow `uv` (ADR 0005): config is intent, the lock is resolution, `sync` makes the disk match the lock. Nothing updates behind the user's back.

Global flags: `--project <dir>`, `-q/--quiet`, `--no-color`.
**Output** (uv's model, ADR 0005). Status lines go to stderr in one style: lowercase, a verb and what it acted on, no full stop (`updated refs.toml`, `created group \`name\``, `updated refs.lock`, `created|moved|removed checkout \`id\``, `updated AGENTS.md`). They appear in that order, one per thing changed. A command that changed nothing says `nothing changed`, and `sync --check` on a current project says `up to date`. `-q` silences status lines; problems (errors, refusals, notes, `out of date:` lines) are always printed. stdout carries data only (`list`), so it is empty for every other command. A hint to run `refs sync` appears only when the project is left incomplete: after a failed `sync` or edit (`... once the problem is fixed`, or for an edit that was written, `fix or remove the broken repo, then run \`refs sync\``) and after an edit with `--no-sync`; the hint is a problem line, so `-q` does not hide it. `init`'s reminders and the `refs.toml` template carry no such hint; the Preamble's line that tells the agent to ask for `refs sync` when `<references_dir>/` is missing stays (§9).

Exit codes: `0` ok, `1` error, `2` usage error, `3` `--check` found something out of date (`sync --check`: the project; `update --check`: a newer release of `refs`). `--check` exits 1, not 3, for a refusal (a foreign or dirty checkout, malformed markers): `sync` would refuse it too. Exit 3 is a recorded divergence from uv (ADR 0005).

| Command | Behaviour |
|---|---|
| `refs init [--here]` | Create `refs.toml` (commented template) if absent, add the exclude rule. It writes no managed block: with no active Repo there is none (§9), and `sync` adds it with the first Repo. Print the linter/formatter reminder and the CLAUDE.md note. Idempotent. Warns and stops if a parent directory already has a `refs.toml`, unless `--here` (§5). |
| `refs add <url> [--id <id>] [--group <g>] [--ref <r>] [--paths <p>…] [--packages <p>…] [--start <p>…] [--description <d>]` | Append a repo at the end of `refs.toml` via `toml_edit`. Id defaults to the URL's last path segment with a trailing `.git` stripped; a taken id is an error pointing at `--id`. An unknown `--group` is created as a bare `[groups.<g>]` (no name, no description) just before the repo, and `add` says so. Then it locks the new repo and syncs the project (below); `--no-sync` only edits. |
| `refs remove <id>` | Remove the repo entry, leaving comments and other entries alone. Remove its group when this was the group's last repo (disabled repos count) and the group has no description, so `add --group x` then `remove` leaves `refs.toml` as it was. Then syncs like `add`; `--no-sync` only edits. |
| `refs enable <id> [--group]`, `refs disable <id> [--group]` | Set or clear `enabled` on a repo, or on a group with `--group` (§6.4). Then syncs like `add`; `--no-sync` only edits. |
| `refs list` | One line per Repo, columns aligned across the listing: `id  url  ref  paths`, with `(whole repo)` when `paths` is empty. Repos sit under their Group's heading, `name (id)`, or just the name when the two are the same; there is no heading at all when the config has no Groups, and an `ungrouped` heading only when it does. A disabled Repo or Group (also one disabled by its Group) is led by `- ` in place of the indent and dimmed on a terminal (not with `NO_COLOR` or a pipe, where the marker remains). Reads the config only: no lock, no git. `--status` adds two columns: the locked SHA (short) and the checkout state (ok / missing / wrong SHA / wrong paths / foreign / not locked / disabled; a wrong SHA or wrong paths that sync would refuse to move because it is dirty reads `wrong SHA, dirty` / `wrong paths, dirty`, while a dirty checkout that matches the lock is `ok`), reading the lock and inspecting each active checkout (no network; a disabled repo is not inspected, and names in the references directory that are not repos are not listed). |
| `refs update [--check]` | Replace this program with the latest release, through axoupdater (cargo-dist's updater, linked in as a library, so `install-updater` stays off). It needs no project. It works only for a `refs` the cargo-dist shell or PowerShell installer put there, as that installer writes the receipt that says where and from which release (`$HOME/.config/refs-cli/` or `$LOCALAPPDATA/refs-cli/`, named for the package, not the binary); with no receipt, or one for a binary in another directory, it exits 1 and says to update the way it was installed. Says `updated refs from <old> to <new>`, or `up to date`. `--check` installs nothing: exit 3 with `out of date: a newer release is available; run \`refs update\``, or `up to date`. The installer's own output is not shown (a failed install reports it). Never run by another command (nothing updates behind the user's back). |
| `refs lock [--upgrade [<id>…]]` | Resolve refs → SHAs and write `refs.lock`. Without `--upgrade`, an already-locked repo keeps its SHA (and `branch`) while its `url`, `ref` and `source` are unchanged; `paths` is not in the lock, so editing `paths` never touches `refs.lock` or moves a floating ref. A repo that was disabled and is enabled again is re-resolved (§6.4). With `--upgrade`, re-resolve floating refs (all, or the given ids), refreshing `branch` for `HEAD` refs. No checkouts touched. Also checks `start` paths (§7.2). |
| `refs sync [--check] [--offline] [--force]` | See below. `--check`: use the existing lock only; never resolve refs, fetch, or write. Exit 3 if the lock, checkouts or blocks are out of date (CI, pre-commit). Floating-ref movement upstream is detected only by `lock --upgrade`. `--offline`: never contact the network; requires a current lock and all objects needed to materialise the desired checkouts, including the blobs for the sparse paths (§7.3 step 1b), already in the cache. Missing required objects are an error. Combined with `--check`, inspect existing state without fetching or materialising; out-of-date state exits 3. `--force`: discard local changes in checkouts (§7.3). |

**Edit then sync.** `add`, `remove`, `enable` and `disable` build the edited `refs.toml` text, then run stage 1 (lock) against that config in memory. The edit is atomic: if stage 1 fails for any Repo (a bad URL or ref, a missing `paths` or `start`), nothing is written, the error names the Repo, the hint says `--no-sync` writes the edit without syncing, and the exit code is 1. If the write of `refs.lock` or `refs.toml` itself fails, the hint says only that `refs.toml` was not changed, since `--no-sync` would not help. The exception is an edit that only shrinks the active set (`remove`, `disable`) against a Lock that has a current pin for every remaining Repo: the lock is pruned of the Repos no longer active, nothing is resolved or verified, so a broken Repo the edit did not touch cannot block it. When stage 1 passes (or the lock is pruned) the edit writes `refs.toml` and `refs.lock`, then run `sync` (stage 2). A stage 2 failure keeps both files and exits 1; `refs sync` retries it. `refs.lock` is written before `refs.toml`, so if the write of `refs.toml` fails the lock has an entry the config lacks, which the next `sync` removes. An edit that changes nothing (enabling what is enabled) writes nothing, says so, and still syncs. `--no-sync` skips all of this: it only edits, and says to run `refs sync`.

`sync` steps:

0. Compute the active set from the config (§6.4). Everything below operates on active repos, except removals (repos no longer active) and the agent file and exclude rule (the project).
1. **Stage 1, lock** (`plan_lock`; ADR 0006). Determine whether the lock is current. A normal sync locks if it is missing or stale: resolve the repos that need it, reuse the other pins, and verify `paths` and `start` of every active repo at its pin against the cache (§7.2), fetching commits and trees only on a miss. Collect all errors; write `refs.lock` only if there are none. With `--offline`, a missing or stale lock is an error and nothing is fetched. With `--check`, skip this stage: a missing or stale lock is out of date (exit 3); do not resolve refs, verify or write the lock.
2. Read the project state: the `Observed` state of every active checkout, the agent files, the exclude rule (present, missing, or no git repo), and the directory names in `references_dir`; each name that is not active is inspected, and an `At` one is removed. A `Foreign` or absent non-active name is ignored (the later `doctor` reports orphans). The old Lock is not consulted.
3. **Stage 2, plan** (`plan_checkouts`) against the lock: an ordered list of actions (removals, materialisations, agent file writes, the exclude rule) plus refusals (`foreign_dir`, `dirty_checkout`, malformed markers) and an info note for a recreated checkout (§7.3). A refusal suppresses the Agent file write (not the exclude rule), so the block never lists a Repo with no checkout. With `--force` the plan replaces the `dirty_checkout` refusal by a removal and materialisation, so `Source` carries no force option.
4. With `--offline`, `materialise` errors, naming an object (§7.3 step 1b), for the repo it cannot serve; other repos proceed as in any failure. There is no all-or-nothing pre-check.
5. Apply the plan in order: ensure the required objects are in the bare blobless cache (§7.3 steps 1 and 1b, no fetching with `--offline`), update the sparse worktrees, prune removed repos, render the block and write it only if it changed, ensure the exclude rule. A failure for one repo does not stop the others; failures are collected. An Agent file is rewritten only if every repo it lists got its checkout (a failed removal of a repo that is no longer active does not hold it back), and each file is written on its own. Exit 1 with all diagnostics.
6. With `--check`, do not apply: exit 3 if the plan holds any action other than an info note (drift), exit 1 if it holds a refusal.

### 10.1 `doctor` checks (deferred, §3.2)

Not in the MVP. Kept as the design for the later command, which reads `Observed` state and the `sync --check` plan rather than re-inspecting.

- git present and at least 2.36.0 (§7.5).
- Project config parse and validate: unknown keys, bad ids, dangling group refs.
- `start` path missing, or not in the checkout (§4, including a root directory when `paths` is set): error.
- A package listed under more than one active repo: info.
- Number of disabled repos and groups: info.
- Lock present and not stale (§10.2).
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

[[repo]]
id = "solid"
source = "git"              # the only kind; any other value is rejected on read
url = "https://github.com/solidjs/solid"
ref = "next"
sha = "1a2b3c4d5e6f…"      # full 40 hex; always a commit (peeled), never a tag object (§7.2)
branch = "main"             # only when ref = "HEAD": the remote's default branch at resolve time (§7.2); display only
paths = ["packages", "documentation"]
```

`source` says where the pin came from. `git` is the only kind in the MVP and the fields are git's (`url`, `ref`, `sha`); the code holds a plain `Pin` and the tag is only checked on read. A later kind would turn the `Pin` back into a union, which is additive and needs no `version` bump. `source` is required in every lock entry (the lock is machine-written). `[repos.<id>]` in `refs.toml` has no `source` key in the MVP; a repo is a git repo.

The lock is **stale** when its entries differ from the active set: an id added or missing, or an entry whose `source`, `url` or `ref` differs from the config. The lock records no `paths` (they are a set, checked by `verify` and applied by the checkout stage), so reordering or editing them does not stale it. This is a direct comparison, not a hash, so a report can name the repo and the field. Enable/disable changes the active set and so stales the lock. Stale does not mean re-resolve: `lock` re-resolves only when `url`, `ref` or `source` changed or the repo is newly active. A floating ref whose upstream branch was renamed is noticed only by `lock --upgrade`, like any floating-ref movement. Group membership and display fields (`description`, `name`, `packages`, `start` and the lock's `branch`) don't stale the lock; they only affect the block on the next `sync`. `sync --check` reports a missing or stale lock as out of date with exit code 3.

---

## 11. Implementation notes

- **Crates (suggested):** `clap` (derive), `serde` + `toml` with the `preserve_order` feature (read; keeps config order, §6.1), `indexmap` with `serde` (ordered `groups`/`repos` maps), `toml_edit` (write), `thiserror` + `miette` (diagnostics, ADR 0002; no `anyhow`), `fd-lock`, `sha2`, `etcetera` or `directories` (XDG paths).
- **Git:** shell out via `std::process::Command`; no libgit2 or gix in the MVP. One module with a thin, testable wrapper. Set `GIT_TERMINAL_PROMPT=0` for non-interactive commands so CI fails fast.
- **Diagnostics:** library errors are `thiserror` enums deriving `miette::Diagnostic` with codes named `refs::<area>::<name>` (e.g. `refs::config::bad_id`; areas `config`, `lock`, `git`, `sync`, `block`, `doctor`). Only `refs.toml` validation errors carry spans, and all of them are collected and reported together (`#[related]` on a wrapper error, one `NamedSource` per child). Config checks for untrusted input get codes too (`refs::config::bad_url` for option-like values and passwords in URLs, `refs::config::bad_ref`, `refs::config::path_not_dir`). Git failures the spec treats specially get their own code (ref not found, ambiguous ref, path missing at SHA, `start` missing or a root directory that is not checked out, foreign directory in `.references/`, dirty checkout, generator mismatch under `--check`, git too old, server refuses fetch-by-SHA); everything else is `refs::git::failed` with the trimmed stderr. Don't parse stderr to detect auth failures. Only the binary enables miette's `fancy` feature.
- **Atomic writes** for `refs.lock` and AGENTS.md: temp file in the same directory, then rename.
- **Paths:** `camino` or careful `Path` handling; forward slashes in rendered output.
- **Windows:** supported, and CI runs on it. Atomic writes retry a rename Windows refuses while another process holds the file; avoid Unix-only assumptions.

### 11.1 Milestones

1. **Git layer:** cache, `lock`, sparse worktrees, exclude rule. Spikes are done (§7). Acceptance: the git lifecycle test (§12) passes.
2. **Block renderer plus `sync`**, with golden tests, including formatter stability.
3. **`init`, `add`, `remove`, `list`** via `toml_edit`.
4. *(Deferred, §3.2)* **`doctor`.**

---

## 12. Tests

- **Resolver fixture:** recorded `ls-remote` output containing an annotated tag (with `^{}` line), a lightweight tag, and a branch/tag name collision; assert the resolved SHAs and the ambiguity error as data, no git. Also an annotated tag whose `^{}` line is absent → peel-locally path.
- **Golden block:** fixture repos built in tests as local `file://` bare repos (with `uploadpack.allowFilter=true`, §7.1); snapshot the rendered block (e.g. `insta`). Cover missing fields, SHA refs, ungrouped repos, no packages anywhere, a `HEAD` ref with and without a `branch` in the lock, a group description (inside the fence), and a non-default `references_dir` in the prose.
- **Formatter stability:** run oxfmt / prettier / dprint over a file containing the block, including `proseWrap=always` and group names at the edge of the allowed character set; output must be unchanged.
- **Diagnostics:** assert on codes, not message strings. `Diagnostic::code()` returns `Option<Box<dyn Display>>`, so the assertion is `d.code().unwrap().to_string()`.
- **Pin reuse:** editing `paths` on a repo with a floating ref keeps its locked SHA and leaves `refs.lock` unchanged; changing `ref` or `url` re-resolves; disable then enable re-resolves.
- **Untrusted input:** a `url` or `ref` starting with `-`, a URL with a password, and a `paths` entry that is a file are rejected with their codes; `ext::` URLs are refused by the protocol allowlist; `--` precedes user-supplied values.
- **Idempotence:** `sync` twice → second run writes nothing (`--check` exits 0).
- **Round-trips:** `add` then `remove` leaves `refs.toml` byte-identical, including comments.
- **Git lifecycle:** add → lock → sync → change ref → lock --upgrade → sync → remove → sync; moved project dir → sync repairs; a checkout deleted by hand (stale registration) → sync recreates it; two projects sharing one cache repo with different sparse paths; concurrent syncs.
- **Blobless and offline (`file://` fixture with `uploadpack.allowFilter=true`; `git daemon` if `file://` misbehaves on CI):** the cache clone is partial; the §7.3 step 1b prefetch brings exactly the root-level and `paths` blobs; `sync --offline` succeeds once they are present and errors, naming the missing objects, when they are not. One opt-in test repeats the blobless check against a real remote.
- **Dangling checkout:** wipe the cache, then `sync` recreates every checkout and prints `refs::sync::recreated`; a directory with no `.git` or a `.git` pointing outside the cache root fails with `refs::sync::foreign_dir`.
- **Dirty checkout:** an untracked file blocks `sync` with `refs::sync::dirty_checkout`; `--force` clears it.
- **Reproducibility:** the committed project config and lock fully determine the generated block; no machine-local config is read.
- **Block safety:** content outside markers untouched; malformed markers refuse to write.
- **`start` validation:** missing path and not-in-checkout errors (including a root directory when `paths` is set); a root-level file is accepted (cone mode) and an ancestor-directory file is rejected; rendered strings with newlines, ```` ``` ```` or marker text are rejected.
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
