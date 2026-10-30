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

- **One hop:** the managed block in `AGENTS.md`, always in context. Per repo: id, ref, short SHA, a description, the packages it documents, and start paths.
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
- Global cache of shallow, blobless bare repos, one fetch per pin (ADR 0010); per-project sparse `git worktree` checkouts in `.references/<repo>/`.
- Managed block in `AGENTS.md`: header line per repo with `Packages:` and `Start:`.
- `.git/info/exclude` rule for the references directory.
- `enabled` flag on groups and repos: a disabled repo behaves as absent for lock, checkouts and the block (§4, §6.4).
- CLI: `init`, `add`, `remove`, `enable`, `disable`, `list`, `sync`, `update`, `self update`.

### 3.2 Deferred (rough priority order)

- Peeling an unpeeled annotated tag locally (fetch the tag object, `rev-parse <sha>^{commit}`); listing every missing object for `sync --offline` (`GIT_NO_LAZY_FETCH=1 git cat-file --batch-check`; never `rev-list --missing=print`); the `REFS_GIT_ALLOW_PROTOCOL` override; `sync --locked` and verbose `--check` diffs (`-v`, `similar`). `--check` already reports a stale lock for CI.

Ranked:

1. `add` infers `packages` from the Manifests the Checkout would hold, read before the edit (ADR 0009, 0011). This is the default way `packages` gets filled; `--packages` (and the other `add` metadata flags) only override it.
2. An optional inline tree as an add-on to the header lines (Vercel favoured one; revisit if evals show a gap).
3. Per-file hints (titles, export names), if evals show a need. Markdown/MDX via a real parser (`pulldown-cmark` or `markdown`), never regex scraping.
4. Linter/formatter/editor exclusion: `sync` reports a Tooling gap for each detected tool or editor that does not exclude `<references_dir>` and prints the lines to add (§7.6); refs never writes those files. `init` and the edit (`add` or `enable`) that gives a Project its first active Repo also print an editor hint.
5. Ref resolution from package lockfiles (`Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, `bun.lock`, `uv.lock`, `poetry.lock` version → git tag). Implemented for `npm:`, `cargo:` and `pypi:` (§7.2, §10): a version given by hand or read from one of those lockfiles maps to a tag. Moving an existing repo is `refs update` (§10).
6. `--json` output.

**CLI alignment with uv (ADR 0005), after the architecture polish and before rank 1.** One batch, no new features before it:

- `-v`/`--color`, short flags where uv has them. (`--offline` is on `sync` and the edit commands; `add` refuses a registry lookup with it.)
- `remove`, `enable` and `disable` take several ids: one edit, one lock, one sync.
- `add` input shorthand: an explicit `npm:<name>`, `cargo:<name>` or `pypi:<name>` prefix, with an optional `@<version>`, asks the registry for the repository (implemented, ADR 0009; `--offline` refuses it); an explicit `gh:owner/repo` (alias `github:`), `gl:group/sub/repo` (alias `gitlab:`) or `bitbucket:owner/repo` prefix (implemented, `registry::forge`, one table of Forges that also tells the registry adapters which hosts are repositories; pure, so `--offline` allows it), never a bare `owner/repo`. A path after the repository (`/tree/main/docs`, GitLab's `/-/…`) is dropped with a stderr warning pointing at `--ref` and `--paths`; GitHub and Bitbucket keep `owner/repo`; GitLab keeps every segment before `/-/`, since a subgroup or repository may be named `wiki` or `tree`. `@` is refused, as `--ref` pins a ref. Input sugar only: `refs.toml` stores the expanded URL. Codeberg has no prefix yet; further hosts later.
- `refs cache clean`/`prune` for the global cache (formerly `gc`/`clean`, rank 6). Required before any release.

**Interactive `add` (ADR 0008), an exception to "no features before it".** It goes before the batch and before rank 1. On a terminal, `add` asks for what it was not given (a bare `refs add` asks for the URL too); `--no-input` (on `add` and `update`) turns it off, and without a terminal nothing is asked. ADR 0011 reshapes it as a summary of the values and one `Customize?` gate, with pickers for `paths` and `start` and the inferred `packages` as a default (`start` is not inferred).

### 3.3 Considered and discarded (with reasons)

| Idea | Why discarded |
|---|---|
| **MCP server** or **query CLI as the primary interface** | Structurally a skill: a decision point the agent often skips (Vercel: 53% with skill uninvoked). |
| **Budgeted inline file tree** (`inline_budget`, water-filling, truncation markers) | Header-only blocks matched tree blocks in evals (runs e, f). Cost: a whole algorithm and a churning block. May return as an optional add-on. |
| **Per-repo `.references/<repo>.md` index files** | 1 incidental read in 11 runs, even with no tree. Agents `ls`/`grep` the checkouts instead. |
| **Global template library** | It adds a second config location, global add/remove behavior, and copy semantics for groups. Projects would still need their own explicit repo definitions for reproducible blocks, so users can add repos directly to each project's config in the MVP. |
| **`docs`/`code`/`examples` path kinds** | Existed only to drive indexing. Now one flat `paths` list. |
| **Auto-updating unpinned repos before answering a query** | Churns the committed block and makes results drift. Updates happen only via `refs update`. |
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
- **Packages:** optional per repo; the names of the packages this repo documents or implements, as written in their Manifests (e.g. `@solidjs/router`). `add` fills them from the repo's Manifests; they may be set by hand (ADR 0009).
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
| Lock | `<project>/refs.lock` | yes | `sync` |
| Global cache | `$XDG_CACHE_HOME/refs` (default `%LOCALAPPDATA%\refs` on Windows, else `~/.cache/refs`) | n/a | `sync` |
| Checkouts | `<project>/<references_dir>/<repo>/`, default `.references/` | no (excluded) | `sync` |
| Managed block | `<project>/AGENTS.md` (configurable) | yes | `init`, `sync` |

Config and lock live at the **project root**, like `pyproject.toml`/`uv.lock`. The references directory stays entirely generated and entirely excluded.

**Project root discovery:** walk up from the current directory to the first directory containing `refs.toml`; if none, the git worktree root (for `init`); `--project <dir>` overrides. `init` run below a directory that already has a `refs.toml` warns and stops instead of silently using the parent; `init --here` creates a nested project in the current directory.

---

## 6. Configuration

### 6.1 Schema

```toml
tooling_ignore = ["prettier"]      # optional; tools to leave out of the tooling gap notes (§7.6): prettier, eslint, black, biome, markdownlint, zed

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
origin = "npm:solid-js"                 # optional; the Package a registry add came from (prefix and name, no version, no URL); only `add` writes it (a registry shorthand, or `--origin` on any add), only `update` reads it (ADR 0012)
paths = ["packages", "documentation"]   # optional; directories; sparse checkout (cone mode); absent = whole repo
packages = ["solid-js", "@solidjs/web"] # optional; rendered as "Packages:"
start = ["documentation/solid-2.0/MIGRATION.md"]  # optional; rendered as "Start:"
enabled = true                          # optional, default true; false = treated as absent (§6.4)
```

`origin` is validated when the config loads (`refs::config::bad_origin`): `npm:`, `cargo:` or `pypi:` and a package name, with no version, URL or whitespace. `sync` and `list` do not read it and the Lock does not record it.

`<id>` for repos is the directory name under `.references/`; validate `[a-z0-9][a-z0-9._-]*`, not `.` or `..`, unique within the config.

Validation: `paths` and `start` entries are relative and `/`-separated (no `\` or `:`), with no `..`, `.` or empty part, so no trailing `/` (`refs::config::bad_path`: git lists what is in `docs/`, so `sync` could not tell what it is); when `paths` is present, every `start` is inside a `paths` entry or a direct child of the repo root (cone mode checks root files out too). With `paths = ["docs/guide"]`, `README.md` and `docs/guide/a.md` are valid `start` values; `docs/README.md`, `src/x.rs` and `docs/guide/../x.md` are not. A `start` is always a file: `sync` rejects a directory with `refs::git::start_not_file` (a directory belongs in `paths`).

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
- **Lock:** a disabled repo has no lock entry. Disabling then running `sync` drops its entry.
- **Checkout:** `sync` removes the checkout of a disabled repo like any repo no longer in the config (§7.3).
- **Block:** a disabled repo has no Entry. A group with no active repos renders no heading (this holds for enabled groups that simply have no repos too).
- **Re-enabling** re-resolves the ref, exactly as uncommenting would. A floating ref may land on a newer commit than before; the old pin isn't kept.
- **Validation still applies** to disabled entries (id, dangling group, `paths`, `start` syntax), but nothing is resolved or fetched for them. `list` shows them marked `disabled`.
- Disabling and groups stay despite having no `uv` counterpart (ADR 0005): Repos and Groups are not colocated in the TOML, so a switch on either is not the same as editing a line.
- `refs disable <id>` / `refs enable <id>` set `enabled` on a repo; `--group` targets a group id instead. Both edit via `toml_edit`, then lock and sync like `add` (§10). `enable` removes the key rather than writing `enabled = true`.

---

## 7. Storage and git mechanics

Shell out to the system `git` binary: it handles auth (credential helpers, SSH agents, private repos) for free. Spikes ran on git 2.55.0. **Minimum git is 2.36.0** (§7.5); `GitSource` checks the version before its first git command.

### 7.1 Global cache

- One **bare, shallow, blobless** repo per normalised URL: `<cache_root>/git/<sha256(normalised_url)[..16]>/`, where `<cache_root>` is `$XDG_CACHE_HOME/refs` (default `%LOCALAPPDATA%\refs` on Windows, else `~/.cache/refs`). Create it with `git init --bare` and the remote's URL, and fetch each Pin into it (below). Store the original URL in a small metadata file beside it.
- URL normalisation: strip trailing `/` and `.git`, lowercase the host. Do not rewrite protocols: `https://github.com/o/r` and `git@github.com:o/r` get separate Cache entries. Unifying them would mean guessing each host's URL mapping.
- Guard every mutation of a cache repo with an exclusive file lock (e.g. `fd-lock`).
- **Shallow by design (ADR 0010).** Each Pin is fetched with `git fetch --no-tags --depth 1 --filter=blob:none origin <sha>`: one commit and its trees, no history, so an entry holds one small pack per Pin. No code uses history. A blobless clone would bring every commit and tree (`rust-lang/cargo`: 26 MB and 5.5 s against 0.2 MB and under a second); `tree:0` is not an option because `ls-tree -r` fails without lazy fetch.
- **The Cache has no fetch refspec** (verified for a bare repository, and kept): a plain `git fetch origin` fetches objects but updates no refs, only `FETCH_HEAD`. So never rely on refs in the cache. Resolve refs with `git ls-remote`; fetch by SHA. A server that refuses a SHA want fails with `refs::git::commit_unavailable`, or with `refs::git::objects_unavailable` for the blobs of step 1b; there is no ref-based fallback until a real host needs one.
- `--filter=blob:none` is verified against a real remote only (GitHub, git 2.55.0): a fetch downloads trees and commits only, `git fetch origin <sha>` works, and a sparse `checkout` downloads just the blobs it needs (10 → 4 missing blobs on a 10-blob test repo). A local `file://` server honours the filter only when the source repo sets `uploadpack.allowFilter=true` (verified on git 2.55.0: partial clone, 11 missing blobs); without it git prints `warning: filtering not recognized by server, ignoring` and silently sends everything. Fixtures therefore set `uploadpack.allowFilter=true` on their bare source repos and assert the Cache entry is partial (`remote.origin.partialclonefilter`), so a silently full fetch fails the test. A `git daemon` fixture behaves the same (verified) and is only needed if `file://` misbehaves on the CI git version. Keep one opt-in test against a real remote (GitHub) for server differences (§12). A host without `allowFilter` (SourceHut) ignores the filter with a warning and exit 0, and sends the blobs with the commit; the partial assertion does not apply to it.

### 7.2 Resolution (`sync`, stage 1)

- **Branch or tag:** one round trip with explicit patterns: `git ls-remote <url> refs/heads/<ref> refs/tags/<ref> 'refs/tags/<ref>^{}'`. The `^{}` pattern matches literally and returns the peeled commit line for an annotated tag (nothing extra for a lightweight one). Prefer tags over branches if both match; error if still ambiguous. Branch/tag name collisions are common in the repos people reference, so this is a real path.
- **`HEAD`:** `git ls-remote --symref <url> HEAD` returns the target branch and its SHA. The lock records the branch name (`branch`, §10.2) so the block can show `@ main` instead of `@ HEAD` (§8.1). If the server sends no `ref:` line (detached remote HEAD), `branch` is omitted.
- **Version to tag (`add npm:<name>@<version>`):** `Source::tags` lists the remote's tags with one `git ls-remote --tags --refs -- <url>` (`--refs` drops the `^{}` lines, so an annotated tag is listed once, by its own name; the URL is checked as in §7.7). This is the one place a bare listing is wanted, since the list is the input to the match, and it runs once per versioned `add`, through `add::pin`, which turns a package and the version given or found into a tag or the default branch, saying what it read and decided as it goes and asking the two questions that can come up (which of several versions, and whether a version with no tag may follow the default branch) through the `Prompter`. It costs one round trip, about 0.7s from GitHub whatever the size (measured with git 2.55.0: `solidjs/solid` 474 tags, `tokio-rs/tokio` 399, `torvalds/linux` 949, against 2178, 5750 and 3830 refs bare), so the filter shrinks the answer, not the wait. `registry::tag::tag_for(name, version, tags)` is pure and matches exactly, never "any tag ending in the version". Order: `{name}@{ver}`, `{name}-v{ver}`, `{name}-{ver}`, `{name}_v{ver}`, `{name}_{ver}` for the full name, then the same for the part after the slash of a scoped npm name (`@scope/pkg` gives `pkg`), then `v{ver}`, then `{ver}`. A tag that names the package comes before the bare `v{ver}`, which in a monorepo may belong to another package (`@babel/core@7.23.2` over `v7.23.2`). The result is a Ref; `resolve` peels it as for any tag.
- **What counts as a version.** After the last `@` that does not start a scope, with one leading `v` ignored: it starts with a digit, holds only letters, digits and `.-+_`, and has no `x` or `X` among its numbers. An empty version, a dist-tag (`latest`) or a range (`^1.2`, `>=1`, `*`, `1.x`) is refused with `refs::registry::bad_version` before the registry is asked.
- **A version with no tag is never pinned silently.** On a terminal `add` says the version is not tagged and asks whether to follow the remote's default branch instead (no cancels the add). Without one it prints a warning on stderr and follows it (`ref` stays absent). With `--no-sync` no tags are listed, so the same warning says the version is not pinned. A version and an explicit `--ref` are refused together (both pin, and they can disagree), before the registry is asked; a `--ref` alone is kept and lists nothing. A shorthand with no version follows `HEAD` and lists nothing. A registry add whose version was given or read from the lockfile has decided its Ref (a tag, or the default branch after the question above), so the `Ref` question of interactive `add` (ADR 0008) is not asked and the pin line adds `use `--ref` to track a branch or choose another tag` (ADR 0012). When nothing was pinned (no version, and the lockfile is missing, unreadable or lacks the package) the `Ref` question is asked, empty meaning the remote's default branch; without a terminal nothing is asked.
- **No version given: the Used version (`add npm:<name>`, `cargo:<name>`, `pypi:<name>`).** With no `@<version>` and no `--ref`, `add` reads the version the project uses from its Package lockfile (`used::answer`), and the tag for it is found as above. `pypi:` reads `uv.lock`, else `poetry.lock`; the binary `bun.lockb` is never read. The lockfile is looked for from the Project directory up to the git worktree top (`worktree::candidate_dirs`; outside a worktree, the Project directory only). The nearest directory with a lockfile of the ecosystem is the one that is read, whether or not it has the package. In one directory npm prefers `pnpm-lock.yaml`, then `yarn.lock`, then `package-lock.json`, then `bun.lock`, and `add` says which others it left unread. `Cargo.lock`: registry packages only, direct when a package of the project (one with no `source`) lists it; `package-lock.json`: version 2 and 3 are direct when the root entry's dependency groups name it, version 1 has no direct versions; `pnpm-lock.yaml`: direct when an importer (or v5/v6 top level) lists it; `yarn.lock` has no direct versions; `bun.lock` (JSON with trailing commas) is found by the real name in each `packages` entry, so an alias is found by the package it stands for, and a version is direct when a workspace's dependency groups name its install path (`ms`, or `<workspace name>/ms` for a copy nested under that workspace), with git, workspace and other non-registry specs left out as in the other formats; `uv.lock`: only a package whose `source` is PyPI's own index (`https://pypi.org/simple`) is a version, and it is direct when a package of the project (a `virtual` or `editable` source) lists it in `dependencies`, `dev-dependencies` or `optional-dependencies`, and a listing that names another index is not it; `poetry.lock`: only a package with no `[package.source]` is a version, and none is direct, since the file does not say what the project asks for. A package from another index is not a PyPI version, however it is named: `pypi:` asks pypi.org for the repository, and another index may serve a different project under that name. Project names are compared with case and `-`, `_`, `.` folded. A version from git, a path, a link or a workspace is not a registry version. Only what a peer suffix adds (`1.2.3(react@18)`) is cleaned from a version; a prerelease (`1.2.3-beta.1`) is kept, as it is how the release is tagged. An alias is found by the package it stands for (`package-lock.json` `name`, yarn Berry `npm:` spec), not by the name it is installed under. In `Cargo.lock` every package with no `source` (the root and the other workspace members) counts as the project. The highest version is the last by semver; one that is not semver sorts after those that are. One version is used; of several, the one the project asks for is, if exactly one is; otherwise a terminal is asked (the highest is the default) and without one `add` fails and says to give `name@version` (`refs::add::several_versions`). A package the lockfile lacks, no lockfile and an unreadable one (the only case the ticket did not list) all say why and follow the remote's default branch (an unreadable one as a warning). Reading is local, so it works under `--offline`; the registry lookup still refuses. Output names the file and the version used.
- **Outline and read (`add`, ADR 0011).** Before an interactive or inferring `add` edits anything, the `outline(url, ref)` method of `Source` resolves the ref, fetches the commit's trees into the Cache and lists every path (`git ls-tree -r --name-only`); `read(url, pin, paths)` fetches the named blobs in one batch (the §7.3 step 1b fetch) and returns their text. Both are keyed by URL, as `tags` is. Which paths are directories, Manifests (at most 200, shallowest first) and `start` candidates is decided in core, under the cone rule of §4.
- **Prefer targeted patterns over a bare `ls-remote`**: a bare listing is ~2000 refs on typical targets (`solidjs/solid` 1967, `torvalds/linux` 3828, ~0.8s each).
- **The lock stores the commit SHA, never a tag object SHA.** For an annotated tag, plain `ls-remote <url> <ref>` returns the tag object and no `^{}` line, and `worktree add` and `ls-tree` peel silently, so a tag SHA in the lock would check out a different commit than the one locked. `sync --check` would then fail forever and §8.1 would render an unlookable SHA, with no error anywhere. Resolve to the commit upstream of the lock.
- **No fallback in the MVP:** if the server omits `^{}` for an annotated tag, `sync` fails with `refs::git::unpeeled_tag` (never store the tag object SHA). Peeling locally is deferred (§3.2).
- A 40-hex ref is taken as-is and verified on fetch. Abbreviated SHAs are rejected. Only 40-hex (SHA-1) object ids are supported; SHA-256 repositories (64 hex) are out of scope for the MVP.
- Verify every configured path and `start` path at the resolved SHA from one list of the files at or below them (`git --literal-pathspecs ls-tree -r --name-only -z <sha> -- <paths> <start>`, nothing when there are neither). A path must be a tree: one with no file below it is an error naming repo, path and SHA (`refs::git::path_missing`), or `refs::git::path_not_dir` when it is listed itself. A `start` path must be listed itself (a file, symlink or submodule) and will be in the checkout (inside `paths`, or a direct child of the root; §4): a missing one is an error (`refs::git::start_missing`), and so is a directory (`refs::git::start_not_file`). Every such error of a Repo is reported, not just the first.

### 7.3 Checkouts (`sync`)

For each locked repo:

1. Ensure the Cache has the SHA.
1b. Prefetch the blobs the checkout will need, under the cache lock, so all network access happens in steps 1 and 1b and `checkout` never fetches lazily. The needed set is the blobs directly in the repo root (`git ls-tree <sha>`, cone mode checks them out), the blobs directly in every ancestor directory of a `paths` entry (`git ls-tree <sha> -- docs/ docs/guide/`; cone mode checks those out too), and every blob under the `paths` (`git ls-tree -r <sha> -- <paths>`), or the whole tree when `paths` is absent, deduplicated by OID and minus those the cache already has (`git cat-file --batch-check`). Fetch them in one round trip with the options git's own lazy fetch uses: `git -c fetch.negotiationAlgorithm=noop fetch origin --no-tags --no-write-fetch-head --recurse-submodules=no --filter=blob:none --stdin`, OIDs on stdin (verified against GitHub, git 2.55.0; without the `noop` negotiation GitHub answers a want for a blob with `did not send all necessary objects`, which a local `file://` server does not reproduce). Prefetching only the `paths` makes a no-lazy checkout fail with `could not fetch <oid> from promisor remote`, and so does leaving out the ancestor files. `--offline` skips the fetches of steps 1 and 1b but not the checks: a commit or a blob the Cache lacks is an error before anything is replaced (`not_cached`, or `object_missing` naming the blob and the Repo), so a move leaves the old Checkout alone. If `checkout` fails on a missing object anyway, `GitSource` removes the directory it just created. `cat-file` lazy-fetches in a partial clone unless `GIT_NO_LAZY_FETCH=1` is set. Run `checkout` with `GIT_NO_LAZY_FETCH=1` as a backstop.
2. If `.references/<repo>/` doesn't exist: run `git -C <cache> worktree prune` first (a deleted checkout, e.g. after `git clean -fdx`, leaves a stale registration that makes `worktree add` fail), then `git -C <cache> worktree add --no-checkout --detach <project>/<references_dir>/<repo> <sha>`, then `git -C <worktree> sparse-checkout set --cone <paths>` (skip when `paths` is absent), then `git -C <worktree> checkout --detach <sha>`.
3. If it exists at another SHA, or with other `paths`: after steps 1 and 1b (so a failure there leaves the old Checkout alone), remove the worktree (`git worktree remove --force`, then `prune`) and create it again from step 2, all under the cache lock. Changing the sparse patterns and the commit in place would need blobs that 1b did not fetch (the new paths at the old commit, or the old paths at the new one); a fresh Checkout reads exactly the prefetched set. A Checkout of another remote (the URL changed) is removed from its own Cache entry first. What a Checkout was made from (its Pin and `paths` as written) is kept in `refs-checkout.toml` in the worktree's admin directory in the cache, so `inspect` reports the Pin with its ref; a record that disagrees with the checked-out commit counts as out of date.
4. If `.references/<repo>/` exists but isn't a worktree of the expected cache repo, stop with `refs::sync::foreign_dir`; never delete a directory refs didn't create. The exception is a **dangling checkout**: its `.git` file points at a gitdir under the cache root that no longer exists (the cache was wiped, e.g. `rm -rf ~/.cache/refs` or a cache cleaner). Checkouts are generated, read-only copies and their history is gone, so `sync` removes the directory and recreates it from step 2, printing `refs::sync::recreated` (info). A directory with no `.git`, or whose `.git` points outside the cache root, is still foreign.

**Sparse config is per-worktree** (verified): the first `sparse-checkout set` enables `extensions.worktreeConfig` in the cache and writes patterns to `worktrees/<name>/config.worktree`. Two worktrees of one cache hold different patterns; nothing leaks across projects. The cache stays bare (its top-level `core.bare = true` is untouched).

Also:

- Remove worktrees for repos no longer active (removed from the config or disabled, §6.4): `git worktree remove --force` (generated, read-only copies). **Unless the checkout is dirty**: if `git status --porcelain` in it shows any modified or untracked file, `sync` refuses to remove it or move it to another SHA and reports `refs::sync::dirty_checkout` naming the files, so the "never delete a directory refs didn't create" rule of step 4 also holds for edits an agent made despite the block. Untracked files count (a `node_modules` created by running a tool inside a checkout blocks `sync` too); the diagnostic says so and names `--force`, which discards them (on `sync`, and on every command that edits `refs.toml`).
- Run `git worktree prune` on each touched cache repo. Prune forgets registrations whose directory is missing, including projects on a currently unmounted drive; their checkouts are recreated by the next `sync`. Accepted.
- Moving a project directory breaks worktree links; `GitSource` repairs the links with `git worktree repair` (silently) or, if that cannot work, `inspect` reports the checkout as dangling and `sync` recreates it.
- **Recreating is announced.** When `sync` recreates a dangling checkout it prints an info-level diagnostic (`refs::sync::recreated`). `-q` silences it. Repairing a moved project and pruning stale registrations (`git worktree repair`, `git worktree prune`) happen silently inside `GitSource`; announcing them (`refs::sync::repaired`, `refs::sync::pruned`) is deferred.
- Ignore submodules and Git LFS content in the MVP.

### 7.4 Server prerequisites

`--filter=blob:none` needs `uploadpack.allowFilter` on the server; fetch-by-SHA needs protocol v2 (default since git 2.26) or `uploadpack.allowReachableSHA1InWant`. GitHub and GitLab satisfy both; a self-hosted `git-http-backend` may not. A server that refuses a commit by its id fails with `refs::git::commit_unavailable`, and one that refuses (or lacks) the blobs of step 1b with `refs::git::objects_unavailable`, since git's refusal and its "not our ref" for a missing object read the same; both helps name the remedy (protocol v2, or `uploadpack.allowReachableSHA1InWant`; a proxy that drops the `Git-Protocol` header forces v0).

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

`GitSource` runs `git --version` once, before its first git command, and fails with `refs::git::too_old` below 2.36.0. This is the only version check.

### 7.6 Exclusion

- `sync` and `init` ensure `/<references_dir>/` is a line in `<git-dir>/info/exclude` (the common git dir for linked worktrees of the project). Found with `git rev-parse --git-common-dir`, so a linked worktree writes to the main repository's file. A project below the repository top gets the rule anchored there, `/<path from the top>/<references_dir>/`. Per-clone, which is fine since every clone runs `sync`.
- If the project isn't in a git repo, `plan` emits an info note and continues; it is not drift.
- Consequence we rely on: default searches (ripgrep) skip `.references/`, so a grep for `useEffect` finds only the user's code. When a search path is passed explicitly (e.g. `.references/solid`), ripgrep searches it anyway. Hidden entries inside a checkout (e.g. `.github/`) stay skipped.
- Tools that do not read `<git-dir>/info/exclude` by themselves are the user's to configure; refs never writes their files. `sync` (and so `add`) reports each one it detects that does not exclude `<references_dir>` as a **Tooling gap**: a note (`refs::tooling::gap`) with the file to edit and the lines to add. Evidence per tool: `docs/research/tool-ignore-support.md`.
  - Tools reported: Prettier (`.prettierignore`), ESLint (the flat config; legacy `.eslintrc*` and `.eslintignore` with no flat config are not reported, as ESLint 8 skips dot-folders by itself), Black (`extend-exclude` in `[tool.black]` of `pyproject.toml`; the snippet has the header only when `pyproject.toml` has no such table), Biome (`files.includes`, or `vcs.enabled` with `vcs.useIgnoreFile`, which makes it read `info/exclude`; `useIgnoreFile` alone does nothing) and markdownlint-cli2 (`ignores`). Each was seen to scan `<references_dir>` with only the Exclude rule in place.
  - Not reported, because they skip `<references_dir>` with only the Exclude rule in place (run for Ruff, ty, dprint, ripgrep and fd, and for one version each of Oxlint (1.90.0) and Oxfmt (0.75.0; the first Oxfmt release that reads `info/exclude` is unverified); Neovim's pickers use ripgrep or fd, but not when they fall back to `find`; documentation or source for Helix, VS Code search and Quick Open): all of those. Not reported, because they skip dot-directories by default: tsc with the default `include` and Stylelint (`.` and `**/*.css`; both run), mypy, pyright and Pylance, Go's `./...`. A `tsconfig.json` whose `include` names the directory, a Stylelint script that globs it, or code that imports from it, is outside this and is not detected. rustfmt and clippy do not walk it. `oxlint`, `tsc` and `stylelint` were reported or planned by earlier versions: `tooling_ignore` still accepts those names and ignores them.
  - A tool is **in use** when a signal of it is in the nearest directory that has one, from the Project directory up to the top of the git worktree (the Project directory only outside a worktree). Signals: a config or ignore file of the tool; its key in `package.json` (`"prettier"`) or its table in `pyproject.toml` (`[tool.black]`); a direct entry in `dependencies` or `devDependencies` of `package.json`; a hook of it in `.pre-commit-config.yaml`. Lockfiles are not read for this (transitive dependencies would report tools the Project does not use), and Python manifests other than the `[tool.*]` tables are not parsed.
  - What covers is per tool, and only what the tool reads for ignores. Prettier: a line of `.prettierignore` that is `<references_dir>` (with or without a leading or trailing `/`, or the path to it from the file's directory; the bare name, with no leading `/`, also matches at any depth); a comment, a negation or a longer path is not. The `.gitignore` of the same directory counts the same way (Prettier reads only the one of its working directory), but not one above it. Its config and `package.json` have no ignore option. ESLint: the flat config naming it (the legacy files cover whatever they contain; a flat config makes ESLint ignore them). Markdownlint-cli2: its config naming it. Biome's config (JSONC) covers when `vcs.enabled` and `vcs.useIgnoreFile` are both `true` (a commented-out setting does not count), or when a string in it that starts with `!` (`!!`) contains the text: a positive `includes` entry only narrows the files and covers nothing. Black's `pyproject.toml` covers only through `exclude`, `extend-exclude` or `force-exclude` of `[tool.black]` (its other files, a pre-commit config, are read as text); only `force-exclude` also hides files passed to Black explicitly, as editors and pre-commit do. A line of the same directory's `.gitignore` that is `<references_dir>` also covers, unless `[tool.black]` has `exclude`, which replaces Black's use of `.gitignore` (`extend-exclude` keeps it). `.gitignore` does not cover markdownlint-cli2: its `gitignore` option is off by default. Zed's settings (JSONC) cover only through an entry of `file_scan_exclusions` that contains the text. A file that does not parse covers nothing. Otherwise nothing is parsed, so a config whose patterns never reach the directory is still reported; use `tooling_ignore`. Every note ends with how to stop it: `(or add "<tool>" to tooling_ignore in refs.toml)`, with the name `tooling_ignore` takes. The editor hint has none. A tool in use with no config to edit gets a note that says to create the file, not to edit one.
  - The snippet gives the directory as seen from the file's directory (`apps/web/.references/**` for a config at the worktree top).
- **Editors.** `info/exclude` is not enough for **Zed** (tested on 1.25.3 on Windows only; Linux and macOS are untested: its project panel, project search and file finder all list the directory). A Zed Project is detected by `.zed/settings.json` in the nearest directory that has one; it is covered when an entry of its `file_scan_exclusions` contains the text of `<references_dir>` (`**/<references_dir>`, a path or an absolute path), and the gap names `"file_scan_exclusions": ["**/<references_dir>", "..."]` (the `"..."` keeps Zed's default list). Other editors are not detected: the file that would show them in use may not exist, and a missing file says nothing. VS Code search and Quick Open, Helix and the ripgrep or fd Neovim pickers already honor `info/exclude`; JetBrains has no setting refs can name from a file, so its advice is only in the hint, and Mark Directory as Excluded is from its docs, not tried.
  - The **editor hint** is one note for all editors, not a Tooling gap: for Zed `file_scan_exclusions`, for JetBrains right-click the directory, Mark Directory as, Excluded. It is printed by `init` when it creates `refs.toml` (a second `init` says nothing), and by an `add` or `enable` that takes the Project from no active Repo to one. It stores no state: it does not appear on `sync`, so a teammate who clones the Project and runs `sync` does not see it.
  - Reported (the gaps) only when a `sync` or `add` that ran got through with no failure or refusal, and at least one Repo is active. `-q` silences it and the hint, it never changes the exit code, never makes a Project out of date, and `sync --check` reports nothing about it.
  - `tooling_ignore` is a top-level list of tool names. An unknown name is a config error (`refs::config::unknown_tool`) that lists the valid ones: `prettier`, `eslint`, `black`, `biome`, `markdownlint`, `zed`, and the retired `oxlint`, `tsc` and `stylelint`, which are accepted and do nothing.

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
- `<ref>` is the ref as configured, except for `HEAD`: if the lock has a `branch` (§10.2), show that (`@ main abc1234`); otherwise show `HEAD`. A lock written before `branch` existed renders `HEAD` until the next `update` re-resolves it.
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
- Claude Code loads `AGENTS.md` only when `CLAUDE.md` doesn't exist, so add `CLAUDE.md` to `agents_files` (the README says the same). `sync` never creates or modifies `CLAUDE.md` unless it's listed there.

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
**Output** (uv's model, ADR 0005). Status lines go to stderr in one style: lowercase, a verb and what it acted on, no full stop (`updated refs.toml`, `created group \`name\``, `updated refs.lock`, `created|moved|removed checkout \`id\``, `updated AGENTS.md`). They appear in that order, one per thing changed. A command that changed nothing says `nothing changed`, and `sync --check` on a current project says `up to date`. `-q` silences status lines; problems (errors, refusals, `out of date:` lines) are always printed, and notes are silenced on a run that got through. stdout carries data only (`list`), so it is empty for every other command. A hint to run `refs sync` appears only when the project is left incomplete: after a failed `sync` or edit (`... once the problem is fixed`, or for an edit that was written, `fix or remove the broken repo, then run \`refs sync\``) and after an edit with `--no-sync`; the hint is a problem line, so `-q` does not hide it. `init`'s reminders and the `refs.toml` template carry no such hint; the Preamble's line that tells the agent to ask for `refs sync` when `<references_dir>/` is missing stays (§9).

Exit codes: `0` ok, `1` error, `2` usage error, `3` `--check` found something out of date (`sync --check`: the project; `self update --check`: a newer release of `refs`). `--check` exits 1, not 3, for a refusal (a foreign or dirty checkout, malformed markers): `sync` would refuse it too. Exit 3 is a recorded divergence from uv (ADR 0005).

| Command | Behaviour |
|---|---|
| `refs init [--here]` | Create `refs.toml` (commented template) if absent, add the exclude rule (outside a git repository it is skipped, with a warning). It writes no managed block: with no active Repo there is none (§9), and `sync` adds it with the first Repo. Print the editor hint (§7.6) and the CLAUDE.md note. Idempotent. Warns and stops if a parent directory already has a `refs.toml`, unless `--here` (§5). |
| `refs add <url> [--id <id>] [--group <g>] [--ref <r>] [--paths <p>…] [--packages <p>…] [--start <p>…] [--description <d>] [--origin <package>]` | Append a repo at the end of `refs.toml` via `toml_edit`. `--force` as on `remove`. `<url>` may be `npm:<name>`, `cargo:<name>` or `pypi:<name>`, each with an optional `@<version>` (ADR 0009): the registry gives the git URL, and `--paths` and `--packages` are written from its `directory` (npm) and the name, unless given; `--packages` is not asked about and the `paths` picker starts from the directory (ADR 0011); with a version (given, or read from the project's Package lockfile), `--ref` defaults to the tag it maps to (§7.2), and a version together with `--ref` is refused; the version is never stored, the prefix and name are kept as the repo's `origin` (ADR 0012; a forge or URL add writes none unless `--origin` gives one, which also replaces a shorthand's: it is checked as `origin` is at load, only records, and reads no lockfile), and `--offline` refuses the lookup. A version that is a word such as `next` is refused with help saying it looks like a tag and showing `npm:<name> --ref next` as the form for a git ref named `next`, which may not be the dist-tag, and a registry add starts the description question from the registry's own description. With no `refs.toml` anywhere up the tree the edit creates an empty one, at the git worktree's top (the current directory outside git), and says `created refs.toml`; nothing is created if the add fails or is cancelled. Id defaults to the URL's last path segment with a trailing `.git` stripped; a taken id is an error pointing at `--id`. An unknown `--group` is created as a bare `[groups.<g>]` (no name, no description) just before the repo, and `add` says so. Then it locks the new repo and syncs the project (below); `--no-sync` only edits, and asks nothing about `packages`, since there is no Checkout to read them from. |
| `refs remove <id>` | Remove the repo entry, leaving comments and other entries alone. Remove its group when this was the group's last repo (disabled repos count) and the group has no description, so `add --group x` then `remove` leaves `refs.toml` as it was. Then syncs like `add`; `--no-sync` only edits. `--force` discards local changes in a Checkout the edit moves or removes; without it such an edit is refused and writes nothing (ADR 0006); it cannot be combined with `--no-sync`. |
| `refs enable <id> [--group]`, `refs disable <id> [--group]` | Set or clear `enabled` on a repo, or on a group with `--group` (§6.4). Then syncs like `add`; `--no-sync` only edits. `--force` discards local changes in a Checkout the edit moves or removes; without it such an edit is refused and writes nothing (ADR 0006); it cannot be combined with `--no-sync`. |
| `refs list` | One line per Repo, columns aligned across the listing: `id  url  ref  paths`, with `(whole repo)` when `paths` is empty. Repos sit under their Group's heading, `name (id)`, or just the name when the two are the same; there is no heading at all when the config has no Groups, and an `ungrouped` heading only when it does. A disabled Repo or Group (also one disabled by its Group) is led by `- ` in place of the indent and dimmed on a terminal (not with `NO_COLOR` or a pipe, where the marker remains). Reads the config only: no lock, no git. `--status` adds two columns: the locked SHA (short) and the checkout state (ok / missing / wrong SHA / wrong paths / foreign / not locked / disabled; a wrong SHA or wrong paths that sync would refuse to move because it is dirty reads `wrong SHA, dirty` / `wrong paths, dirty`, while a dirty checkout that matches the lock is `ok`), reading the lock and inspecting each active checkout (no network; a disabled repo is not inspected, and names in the references directory that are not repos are not listed). |
| `refs update [id…] [--ref <r>] [--force] [--list]` | Move the named Repos (none, in a terminal: a multi-select of every enabled Repo after a `checking N repos…` status line, labelled `id  current → new`, `id  up to date` with the note, `id  current → choose version…` (asked only if picked), or `id  (cannot check: why)`, with `(uncommitted changes)` appended to a dirty mover and nothing ticked; picking is the consent, nothing picked changes nothing; without a terminal it is `--list`, with a `listing only` status line) in one edit of `refs.toml`, one Lock update and one sync, then say `id  old → new` per Repo. Disabled and unknown ids refuse the whole run before any remote is asked (`refs::update::unknown_repo`); `update` has no `--offline` (every answer comes from a remote). What a Repo moves to depends on its Ref (ADR 0012): a **tag Ref** (listed by the remote's `tags`) of a Repo with an Origin moves to the Version tag of the version the project's Package lockfile uses (a downgrade too; several versions are asked, or skipped without a terminal; a version with no Version tag is a note and the Repo stays); when no lockfile answers (no Origin, no lockfile, or the Package is not in it) it moves to the newest tag of its own kind (`registry::tag::newest`: the highest semver version among the tags with an equal prefix, prereleases ignored for a stable tag, a major bump allowed; a tag with no version is a note pointing at `--ref`); a commit is a note pointing at `--ref`; a **branch Ref** (no Ref, or one that is not a tag) moves in the Lock alone, with `refs.toml` byte-identical, when the remote's commit differs from the Lock's. `--ref <r>` (one Repo; with more it is a usage error, `refs::update::ref_needs_one_repo`) sets the Ref without reading a lockfile and never asks. The Registry is never asked, and `description` is never touched. A mover whose new tree lacks a `path` (as a directory) or a `start` file (`Outline::gone`, over the files of the new tree; advisory, `verify` still decides) shows what is gone and asks `Update?` (default no): N leaves the Repo and its Ref as they are and exits 0, yes re-asks only the broken fields from `refs.toml` with the valid entries ticked and the missing ones unticked, and `packages` the new Manifests lack are kept and warned about. Without a terminal a break skips that Repo. A Ref gone from the remote is asked again, prefilled, in a terminal, and is an error naming `--ref` without one. Esc edits nothing. A Repo that cannot be assessed, whose new Ref the remote lacks, or whose Checkout has uncommitted changes is skipped before the edit and named (`--force` moves a dirty one and discards the changes); the rest go through, and the run ends `N repos were not updated` and exits 1. Notes are status lines (`-q` hides them); skips and that line are problems. Stage 1 still resolves the whole edited config (§10.2), so a failure the remote causes after the assessment blocks the run. `--list` (not with `--force`) is the dry run: it asks the same remotes and prints on stdout, per Repo, the label, then what the new Ref would break and, for a dirty Checkout, `would be skipped: uncommitted changes (files)`; a `choose version…` is listed with the versions found and never asked; it needs no terminal and writes nothing. |
| `refs self update [--check]` | Replace this program with the latest release, through axoupdater (cargo-dist's updater, linked in as a library, so `install-updater` stays off). It needs no project. It works only for a `refs` the cargo-dist shell or PowerShell installer put there, as that installer writes the receipt that says where and from which release (`$HOME/.config/refs-cli/` or `$LOCALAPPDATA/refs-cli/`, named for the package, not the binary); with no receipt, or one for a binary in another directory, it exits 1 and says to update the way it was installed. Says `updated refs from <old> to <new>`, or `up to date`. `--check` installs nothing: exit 3 with `out of date: a newer release is available; run \`refs self update\``, or `up to date`. The installer's own output is not shown (a failed install reports it). Never run by another command (nothing updates behind the user's back). |
| `refs sync [--check] [--force]` | See below. `--offline` is an option of `sync` and of the edit commands (`add`, `remove`, `disable`, `enable`). `--check`: use the existing lock only; never resolve refs, fetch, or write. Exit 3 if the lock, checkouts or blocks are out of date (CI, pre-commit). Floating-ref movement upstream is detected only by `update --list`. `--offline`: never contact the network; requires a current lock and all objects needed to materialise the desired checkouts, including the blobs for the sparse paths (§7.3 step 1b), already in the cache. Missing required objects are an error. Combined with `--check`, inspect existing state without fetching or materialising; out-of-date state exits 3. `--force`: discard local changes in checkouts (§7.3). |

**Edit then sync.** `add`, `remove`, `enable`, `disable` and `update` build the edited `refs.toml` text, then run stage 1 (lock) against that config in memory. The edit is atomic: if stage 1 fails for any Repo (a bad URL or ref, a missing `paths` or `start`), or stage 2 would refuse it (a dirty Checkout, a foreign directory, malformed markers; `--force` discards a dirty one) or cannot be observed, nothing is written, the error names the Repo, the hint says `--no-sync` writes the edit without syncing, and the exit code is 1. If the write of `refs.lock` or `refs.toml` itself fails, the hint says only that `refs.toml` was not changed, since `--no-sync` would not help. The exception is an edit that only shrinks the active set (`remove`, `disable`) against a Lock that has a current pin for every remaining Repo: the lock is pruned of the Repos no longer active, nothing is resolved or verified, so a broken Repo the edit did not touch cannot block it. When stage 1 passes (or the lock is pruned) and stage 2 has no refusal, the edit writes `refs.toml` and `refs.lock`, then applies the stage 2 plan it checked. A stage 2 failure (network, disk) keeps both files and exits 1; `refs sync` retries it. `refs.lock` is written before `refs.toml`, so if the write of `refs.toml` fails the lock has an entry the config lacks, which the next `sync` removes. An edit that changes nothing (enabling what is enabled, or an `update` that only moves Lock entries) writes no `refs.toml` but takes the same path otherwise: it observes, locks, honours the `update` ids that move a Lock entry alone, and a refusal blocks it. `--no-sync` skips all of this: it only edits, and says to run `refs sync`.

`sync` steps:

0. Compute the active set from the config (§6.4). Everything below operates on active repos, except removals (repos no longer active) and the agent file and exclude rule (the project).
1. **Stage 1, lock** (`plan_lock`; ADR 0006). Determine whether the lock is current. A normal sync locks if it is missing or stale: resolve the repos that need it, reuse the other pins, and verify `paths` and `start` of every active repo at its pin against the cache (§7.2), fetching commits and trees only on a miss. Collect all errors; write `refs.lock` only if there are none. With `--offline`, a missing or stale lock is an error and nothing is fetched. With `--check`, skip this stage: a missing or stale lock is out of date (exit 3); do not resolve refs, verify or write the lock.
2. Read the project state: the `Observed` state of every active checkout, the agent files, the exclude rule (present, missing, or no git repo), and the directory names in `references_dir`; each name that is not active is inspected, and an `At` one is removed. A `Foreign` or absent non-active name is ignored. The old Lock is not consulted.
3. **Stage 2, plan** (`plan_checkouts`) against the lock: an ordered list of actions (removals, materialisations, agent file writes, the exclude rule) plus refusals (`foreign_dir`, `dirty_checkout`, malformed markers) and an info note for a recreated checkout (§7.3). A refusal suppresses the Agent file write (not the exclude rule), so the block never lists a Repo with no checkout. With `--force` the plan replaces the `dirty_checkout` refusal by a removal and materialisation, so `Source` carries no force option.
4. With `--offline`, `materialise` errors, naming an object (§7.3 step 1b), for the repo it cannot serve; other repos proceed as in any failure. There is no all-or-nothing pre-check.
5. Apply the plan in order: ensure the required objects are in the bare blobless cache (§7.3 steps 1 and 1b, no fetching with `--offline`), update the sparse worktrees, prune removed repos, render the block and write it only if it changed, ensure the exclude rule. A failure for one repo does not stop the others; failures are collected. An Agent file is rewritten only if every repo it lists got its checkout (a failed removal of a repo that is no longer active does not hold it back), and each file is written on its own. Exit 1 with all diagnostics.
6. With `--check`, do not apply: exit 3 if the plan holds any action other than an info note (drift), exit 1 if it holds a refusal.

### 10.1 Checks considered, not planned

There is no `refs doctor` (#5 is `wontfix`). A check on this list that is wanted becomes an advisory on `sync`, as the Tooling gap did (§7.6). The list stays as the inventory of what could be checked.

- git present and at least 2.36.0 (§7.5).
- Project config parse and validate: unknown keys, bad ids, dangling group refs.
- `start` path missing, a directory, or not in the checkout (§4): error.
- A package listed under more than one active repo: info.
- Number of disabled repos and groups: info.
- Lock present and not stale (§10.2).
- Each checkout exists, is a worktree of the right cache repo, is at the locked SHA, and has the right sparse patterns. A dirty checkout: warn.
- Orphan directories in `references_dir` not matching any repo.
- Cache worktree registrations whose directory is missing: info (the next `sync` prunes them and recreates this project's; §7.3).
- Exclude rule present.
- Server ignored the blob filter (cache clone is a full clone): info (§7.4).
- Managed markers present and well-formed in each `agents_files` entry.
- Reminder that linters/formatters/type checkers may scan `references_dir` (`sync` already reports the ones it detects, §7.6; `init` prints the editor hint).
- Submodules or LFS pointers detected in a checkout: info.

Each finding is `ok` / `warn` / `error` with a stable code and a one-line fix. These are miette diagnostics (ADR 0002): `warn` is `Severity::Warning`, `error` is the default severity, the fix is `help`. Exit 1 if any error.

### 10.2 Lock file format

TOML, sorted by repo id, stable formatting:

```toml
# Generated by refs. Do not edit.
version = 1

[[repo]]
id = "solid"
url = "https://github.com/solidjs/solid"
ref = "next"
sha = "1a2b3c4d5e6f…"      # full 40 hex; always a commit (peeled), never a tag object (§7.2)
branch = "main"             # only when ref = "HEAD": the remote's default branch at resolve time (§7.2); display only
paths = ["packages", "documentation"]
```

The fields are git's (`url`, `ref`, `sha`); a repo is a git repo, and `[repos.<id>]` in `refs.toml` has no `source` key. A later kind of source would add a tag then, which is additive and needs no `version` bump.

The lock is **stale** when its entries differ from the active set: an id added or missing, or an entry whose `url` or `ref` differs from the config. The lock records no `paths` (they are a set, checked by `verify` and applied by the checkout stage), so reordering or editing them does not stale it. This is a direct comparison, not a hash, so a report can name the repo and the field. Enable/disable changes the active set and so stales the lock. Stale does not mean re-resolve: `sync` re-resolves only when `url` or `ref` changed or the repo is newly active. A floating ref whose upstream branch was renamed is noticed only by `update --list`, like any floating-ref movement. Group membership and display fields (`description`, `name`, `packages`, `start` and the lock's `branch`) don't stale the lock; they only affect the block on the next `sync`. `sync --check` reports a missing or stale lock as out of date with exit code 3.

---

## 11. Implementation notes

- **Crates (suggested):** `clap` (derive), `serde` + `toml` with the `preserve_order` feature (read; keeps config order, §6.1), `indexmap` with `serde` (ordered `groups`/`repos` maps), `toml_edit` (write), `thiserror` + `miette` (diagnostics, ADR 0002; no `anyhow`), `fd-lock`, `sha2`, `etcetera` or `directories` (XDG paths).
- **Git:** shell out via `std::process::Command`; no libgit2 or gix in the MVP. One module with a thin, testable wrapper. Set `GIT_TERMINAL_PROMPT=0` for non-interactive commands so CI fails fast. Set `LC_ALL=C` for every git command: some failures are told apart by git's English wording (`not our ref`, `could not fetch … from promisor remote`), which a translation changes.
- **Diagnostics:** library errors are `thiserror` enums deriving `miette::Diagnostic` with codes named `refs::<area>::<name>` (e.g. `refs::config::bad_id`; areas `config`, `lock`, `git`, `sync`, `block`, `io`; a file that cannot be read or written is `refs::io::read_failed` or `refs::io::write_failed`, whichever file it is). Only `refs.toml` validation errors carry spans, and all of them are collected and reported together (`#[related]` on a wrapper error, one `NamedSource` per child). Config checks for untrusted input get codes too (`refs::config::bad_url` for option-like values and passwords in URLs, `refs::config::bad_ref`, `refs::config::path_not_dir`). Git failures the spec treats specially get their own code (ref not found, ambiguous ref, path missing at SHA, `start` missing or a directory, foreign directory in `.references/`, dirty checkout, generator mismatch under `--check`, git too old, server refuses fetch-by-SHA); everything else is `refs::git::failed` with the trimmed stderr. Don't parse stderr to detect auth failures. Only the binary enables miette's `fancy` feature.
- **Atomic writes** for `refs.lock` and AGENTS.md: temp file in the same directory, then rename.
- **Paths:** `camino` or careful `Path` handling; forward slashes in rendered output.
- **Windows:** supported, and CI runs on it. Atomic writes retry a rename Windows refuses while another process holds the file; avoid Unix-only assumptions.

### 11.1 Milestones

1. **Git layer:** cache, resolution, sparse worktrees, exclude rule. Spikes are done (§7). Acceptance: the git lifecycle test (§12) passes.
2. **Block renderer plus `sync`**, with golden tests, including formatter stability.
3. **`init`, `add`, `remove`, `list`** via `toml_edit`.

---

## 12. Tests

- **Tag mapper:** `tag_for` over tag lists recorded from real repositories (`tokio-1.40.0`, `tokio-util-0.7.12`, bare `0.4.22`, `v1.0.200`, `@changesets/cli@2.27.0`, `@babel/core@7.23.2` over `v7.23.2`, a version with no tag, no tags), as data, no git. `Source::tags` is a row of the `Source` contract, run against the fake and git.
- **Package lockfiles:** `used_versions` per format over trimmed real lockfiles (`Cargo.lock`, `package-lock.json` v1 and v3 with aliases, links and git packages; `pnpm-lock.yaml` v5 to v9; `yarn.lock` v1 and Berry; `bun.lock`; `uv.lock`, including a workspace and other indexes; `poetry.lock`), as data; the pnpm and yarn cases are opensrc's. `used::answer` over temp directories (nearest wins, stops at the worktree top, npm and Python preference). `add` through `cli` with a fake registry and source, and a scripted prompter for the several-versions question.
- **Resolver fixture:** recorded `ls-remote` output containing an annotated tag (with `^{}` line), a lightweight tag, and a branch/tag name collision; assert the resolved SHAs and the ambiguity error as data, no git. Also an annotated tag whose `^{}` line is absent → peel-locally path.
- **Golden block:** fixture repos built in tests as local `file://` bare repos (with `uploadpack.allowFilter=true`, §7.1); snapshot the rendered block (e.g. `insta`). Cover missing fields, SHA refs, ungrouped repos, no packages anywhere, a `HEAD` ref with and without a `branch` in the lock, a group description (inside the fence), and a non-default `references_dir` in the prose.
- **Formatter stability:** run oxfmt / prettier / dprint over a file containing the block, including `proseWrap=always` and group names at the edge of the allowed character set; output must be unchanged.
- **Diagnostics:** assert on codes, not message strings. `Diagnostic::code()` returns `Option<Box<dyn Display>>`, so the assertion is `d.code().unwrap().to_string()`.
- **Pin reuse:** editing `paths` on a repo with a floating ref keeps its locked SHA and leaves `refs.lock` unchanged; changing `ref` or `url` re-resolves; disable then enable re-resolves.
- **Untrusted input:** a `url` or `ref` starting with `-`, a URL with a password, and a `paths` entry that is a file are rejected with their codes; `ext::` URLs are refused by the protocol allowlist; `--` precedes user-supplied values.
- **Idempotence:** `sync` twice → second run writes nothing (`--check` exits 0).
- **Round-trips:** `add` then `remove` leaves `refs.toml` byte-identical, including comments.
- **Git lifecycle:** add → sync → change ref → sync → update → remove → sync; moved project dir → sync repairs; a checkout deleted by hand (stale registration) → sync recreates it; two projects sharing one cache repo with different sparse paths; concurrent syncs.
- **Blobless and offline (`file://` fixture with `uploadpack.allowFilter=true`; `git daemon` if `file://` misbehaves on CI):** the Cache entry is partial; the §7.3 step 1b prefetch brings exactly the root-level and `paths` blobs; `sync --offline` succeeds once they are present and errors, naming the missing objects, when they are not. One opt-in test repeats the blobless check against a real remote.
- **Dangling checkout:** wipe the cache, then `sync` recreates every checkout and prints `refs::sync::recreated`; a directory with no `.git` or a `.git` pointing outside the cache root fails with `refs::sync::foreign_dir`.
- **Dirty checkout:** an untracked file blocks `sync`, and an edit that would move or remove the checkout, with `refs::sync::dirty_checkout`; `--force` clears it. A refused edit writes nothing.
- **Reproducibility:** the committed project config and lock fully determine the generated block; no machine-local config is read.
- **Block safety:** content outside markers untouched; malformed markers refuse to write.
- **`start` validation:** missing path, directory and not-in-checkout errors; a root-level file is accepted (cone mode) and an ancestor-directory file is rejected; rendered strings with newlines, ```` ``` ```` or marker text are rejected.
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

Resolved: the lock records the default branch for `HEAD` refs and the block shows it (§8.1, §10.2); a `paths` edit never moves a pin (§10); blobs are prefetched so `--offline` is checkable (§7.3); group text renders inside the fence (§8.1); `url`/`ref` are validated and git runs with a protocol allowlist (§6.1, §7.7). Minimum git is 2.36.0 (§7.5). A package listed under several repos is rendered as written (each repo's line lists it). A repo has exactly one group; there is no multi-group membership (a group is only a block heading plus a "when to look here" description, and lookup goes through `Packages:`).
