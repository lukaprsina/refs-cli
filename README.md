# refs

Local reference repos for coding agents.

`refs` clones the git repos your project depends on, checks out only the parts that matter at a pinned commit into `.references/`, and writes a short block into `AGENTS.md`: what exists, which packages each repo documents, where to start. The agent then reads real, current source with its own read and grep tools. No MCP server, no query API.

Anything an agent has to decide to call often goes uncalled; the block sits in `AGENTS.md`, so it is always in context. Pinned commits make it reproducible, sparse checkouts keep it small.

## Install

Needs git 2.36 or newer (this excludes Ubuntu 22.04).

```sh
# macOS, Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/lukaprsina/refs-cli/releases/latest/download/refs-cli-installer.sh | sh
```

```powershell
# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/lukaprsina/refs-cli/releases/latest/download/refs-cli-installer.ps1 | iex"
```

With a Rust toolchain: `cargo binstall refs-cli` (prebuilt binary) or `cargo install refs-cli` (builds from source).

Update with `refs self update`; it works for installs made by the installers above, not by cargo. If GitHub rate-limits the lookup, set `AXOUPDATER_GITHUB_TOKEN`, e.g. `AXOUPDATER_GITHUB_TOKEN=$(gh auth token) refs self update`.

Release artifacts carry GitHub artifact attestations:

```sh
gh attestation verify <file> -R lukaprsina/refs-cli
```

## Quick start

```sh
refs add
```

`refs add` creates `refs.toml` if there is none. In a terminal it asks for the URL (or a shorthand such as `gh:owner/repo` or `npm:name`) and the ref, then looks at the repo: a bad URL or ref fails here, before anything is edited. It reads the repo's manifests (`package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`) for the package names and a description, shows one summary of what it will write, and asks `Customize?`. Enter adds as shown; yes asks for each value in turn, starting from the summary's. It prints the equivalent command line before it syncs:

```text
$ refs add gh:solidjs/solid
> Ref (empty: the remote's default branch) next
id          solid
ref         next (a1b2c3d)
paths       (everything)
start       (none)
packages    @solidjs/signals, @solidjs/web, solid-js
description Solid 2.0 core packages and 2.0 design docs
group       (none)
> Customize? No
equivalent: refs add https://github.com/solidjs/solid --ref next --description "Solid 2.0 core packages and 2.0 design docs" --packages @solidjs/signals @solidjs/web solid-js
created refs.toml
updated refs.lock
created checkout `solid`
updated AGENTS.md
```

A registry shorthand (`npm:name`) pins the version your lockfile or `@version` gives and does not ask for the ref (`--ref` overrides the pin); with no lockfile pin it asks, and an empty answer follows the default branch. `npm:name@next` is refused: `next` looks like a tag, and `npm:name --ref next` follows a git ref named `next`, which may not be the dist-tag.

A registry add also stores the package it came from, as `origin = "npm:solid-js"` in the repo's table (a version is never part of it). A repo added by URL or `gh:` has none. `--origin <package>` records one on any add (`npm:solid-js`, `cargo:serde` or `pypi:requests`, with no version), and replaces the one a shorthand gives. It only records: nothing is read or pinned. An origin is the Package whose lockfile entry the repo follows, so a package whose version scheme differs from the repo's tags will not match them. To attach one to a repo added earlier, add the line to `refs.toml` by hand.

Flags skip the questions. Without a terminal, or with `--no-input`, nothing is asked, so scripts pass a URL and flags.

The result is in `refs.toml`, which you can also write by hand:

```toml
[settings]  # optional; these are the defaults
references_dir = ".references"
agents_files = ["AGENTS.md"]

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
```

`refs sync` (which `add` runs for you) checks out the repos and writes the block into `AGENTS.md`:

````markdown
<!-- BEGIN:refs -->

## Reference repos

```text
Source and docs for some dependencies are checked out read-only in `.references/`, pinned in `refs.lock`. …
```

### SolidJS 2.0

```text
Solid 2.0 release candidates, router and docs. Newer than your training data; many APIs differ from Solid 1.x.

[solid @ next ee49b3e] Solid 2.0 core packages and 2.0 design docs. Packages: solid-js, @solidjs/web, @solidjs/signals. Start: documentation/solid-2.0/MIGRATION.md.
```

<!-- END:refs -->
````

Commit `refs.toml`, `refs.lock` and `AGENTS.md`. `.references/` is excluded through `.git/info/exclude`.

## Commands

| Command | Does |
|---|---|
| `refs init` | Create `refs.toml` (with a commented template) and the git exclude rule |
| `refs add [url]` | Add a repo, lock it, sync. `url` can be `gh:owner/repo` or `gl:group/sub/repo` (also `github:`, `gitlab:`, `bitbucket:`; a path after the repository is ignored with a note), or `npm:<name>`, `cargo:<name>` or `pypi:<name>`, each with an optional `@<version>`: the registry says where the package lives. With no version, `add` reads the one your lockfile uses (`Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, `bun.lock`, `uv.lock`, `poetry.lock`) and pins its tag. Asks for what is missing in a terminal; `--no-input` turns that off, `--no-sync` only edits |
| `refs remove <id>` | Remove a repo |
| `refs disable` / `enable` | Switch a repo or group (`--group`) off or on without removing it |
| `refs list` | Show the repos; `--status` adds the lock and checkout state |
| `refs lock` | Resolve refs to commits in `refs.lock`; `--upgrade` moves floating refs |
| `refs sync` | Make checkouts and the block match `refs.lock`; `--check` exits 3 when out of date, for CI |
| `refs update [id…]` | Move repos (no ids, in a terminal: pick them from a list of `current → new`, nothing ticked): a pinned repo added from `npm:`, `cargo:` or `pypi:` to the version your lockfile uses now, a branch to the remote's newest commit. `--ref <r>` sets the ref of one repo; `--force` moves a repo whose checkout has changes; `--list` prints what would move and what it would break, with no terminal and no changes |
| `refs self update` | Replace this program with the latest release |

Flags: `refs <command> --help`. `--offline` (any command) never contacts the network. Exit codes: 0 ok, 1 error, 2 usage, 3 out of date (`--check`).

## Notes

- Linters, formatters and type checkers may scan `.references/`. `refs sync` and `refs add` say which of Prettier, ESLint, Oxlint and tsc do not exclude it and what to add; refs never edits their files. Not detected: Ruff, Biome, Stylelint, and editors (`files.exclude`, `search.exclude`); exclude `.references/` there yourself. Silence one with `tooling_ignore = ["tsc"]` at the top of `refs.toml`.
- `rg` skips `.references/` by default; pass the path (`rg foo .references/solid`).
- Checkouts are read-only. `sync` and every command that edits `refs.toml` refuse to touch one with local changes (an edit that is refused writes nothing); `--force` discards them.
- Claude Code ignores `AGENTS.md` if a `CLAUDE.md` exists; add `CLAUDE.md` to `agents_files`.

## Thanks

The registry lookups, the cleaning of repository URLs and the reading of `pnpm-lock.yaml` and `yarn.lock` build on [opensrc](https://github.com/vercel-labs/opensrc) by Vercel (Apache-2.0; see `NOTICE`).
