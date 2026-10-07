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

Update with `refs update`; it works for installs made by these installers. If GitHub rate-limits the lookup, set `AXOUPDATER_GITHUB_TOKEN`, e.g. `AXOUPDATER_GITHUB_TOKEN=$(gh auth token) refs update`.

Release artifacts carry GitHub artifact attestations:

```sh
gh attestation verify <file> -R lukaprsina/refs-cli
```

## Quick start

```sh
refs init
refs add
```

In a terminal `refs add` asks for what you didn't pass: the URL, id, group (type to filter, or a new name to create one), ref, paths, packages, start and description. It then locks the repo and syncs, and prints the equivalent command line:

```text
> Repository URL https://github.com/solidjs/solid
> Id solid
> Link to a group? Yes
? Group (type to filter, or a new name to create one)
  solidjs-2
[↑↓ to move, tab to autocomplete, enter to submit]
```

Flags skip the questions. Without a terminal, or with `--no-input`, nothing is asked, so scripts pass a URL and flags.

The result is in `refs.toml`, which you can also write by hand:

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
| `refs init` | Create `refs.toml` and the git exclude rule |
| `refs add [url]` | Add a repo, lock it, sync. Asks for what is missing in a terminal; `--no-input` turns that off, `--no-sync` only edits |
| `refs remove <id>` | Remove a repo |
| `refs disable` / `enable` | Switch a repo or group (`--group`) off or on without removing it |
| `refs list` | Show the repos; `--status` adds the lock and checkout state |
| `refs lock` | Resolve refs to commits in `refs.lock`; `--upgrade` moves floating refs |
| `refs sync` | Make checkouts and the block match `refs.lock`; `--check` exits 3 when out of date, for CI |
| `refs update` | Replace this program with the latest release |

Flags: `refs <command> --help`.

## Notes

- Linters, formatters and type checkers may scan `.references/`; excluding it is up to you.
- Claude Code reads `AGENTS.md` only when there is no `CLAUDE.md`; if you have one, have it import `@AGENTS.md`.
