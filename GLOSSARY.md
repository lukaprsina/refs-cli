# refs

A tool that pins the upstream repositories a project depends on, makes the relevant parts of them readable on disk, and tells a coding agent what exists and where to look.

## Language

### What is referenced

**Project**:
A directory holding a `refs.toml`, together with its Lock, Checkouts and Agent files.
_Avoid_: Workspace, package, root

**Repo**:
One upstream git repository at one ref, identified within a Project by its id, not its URL, e.g. `solid`. Two Repos may share a URL with different Refs or Paths. A docs site kept in a separate repository is just another Repo.
_Avoid_: Reference, dependency, source, library

**Group**:
A named cluster of Repos that share a "when to look here" description. A Repo belongs to at most one Group.
_Avoid_: Category, collection, bundle

**Active**:
A Repo is active when both it and its Group are enabled. Inactive Repos are treated as absent everywhere except listing and validation.
_Avoid_: Live, on, selected

**Paths**:
The repo-relative locations of a Repo that are made readable. Absent means the whole Repo.
_Avoid_: Docs, sources, includes

**Packages**:
The names of the packages a Repo documents or implements, in any language: read from its Manifests or set by hand. Names are taken as written, with no ecosystem qualifier, so they are not always the name used in an import (a crate `foo-bar` is imported as `foo_bar`).
_Avoid_: Exports, libraries, modules

**Manifest**:
A file inside a Repo that names the package it defines, such as `package.json` or `Cargo.toml`. Only the Manifests within a Repo's Paths, and the files at the root and along the way to each path, exist in its Checkout.
_Avoid_: Package file, metadata file

**Registry**:
A package index (npm, crates.io, PyPI) that `add` asks for a package's repository. Named in `add` by a prefix (`npm:`, `cargo:`, `pypi:`); the Repo that is stored has the git URL, never the prefix in it, and the Package it came from as its Origin.
_Avoid_: Source, Cache

**Package**:
A name published on a Registry: the ecosystem and the name together (`npm:@scope/name`, `cargo:serde`). It is what a shorthand asks the Registry about and what a Package lockfile is searched for. Not the same as **Packages**, which are the names a Repo documents, read from its Manifests with no ecosystem.
_Avoid_: Release, Dependency

**Origin**:
The Package a Repo was added from, kept so `refs update` can read the Package lockfile again and name the Version tag (with no Origin, or no lockfile answer, it falls back to the newest tag of its own kind). Absent for a Repo added by URL or Forge, unless `add --origin <package>` records one: the Package whose lockfile entry the Repo follows, which only records and reads no lockfile. It is only intent: the Lock never records it and `sync` never reads it.
_Avoid_: Provenance, Source

**Forge**:
A code host (GitHub, GitLab, Bitbucket, Codeberg) that `add` names with a prefix (`gh:`, `gl:`, `bitbucket:`; Codeberg has none yet) and the path of a repository on it; the Repo that is stored has the https URL, never the prefix. It asks nothing, unlike a Registry.
_Avoid_: Registry, Provider

**Start**:
Files inside a Repo's Paths that are worth reading first.
_Avoid_: Entry points, hints

**Ref**:
A branch, tag or full commit SHA naming what to follow in a Repo. `refs update` treats them by what the Ref is: a **tag Ref** is a Ref the remote lists as a tag (it follows the Package lockfile when the Repo has an Origin, and the newest tag of its own kind, the same prefix and the highest semver version, when no lockfile answers); a **branch Ref** is no Ref, or one that is not a tag (it follows the remote, and moves when the remote's commit differs from the Lock's); a commit has no news.
_Avoid_: Version, revision

**Package lockfile**:
A file written by a package manager (`Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, `bun.lock`, `uv.lock`, `poetry.lock`) that records the versions a project resolved. refs only reads it, and it is not the Lock.
_Avoid_: Lock, lockfile

**Version tag**:
The tag of a Repo that a package's version was released as, found by exact match on the names that package's releases use (`name@1.2.3`, `name-v1.2.3`, `v1.2.3`, ...). It becomes the Ref of a Repo added as `npm:name@1.2.3`, or with a Used version found in the Package lockfile, and the new Ref of a Repo that `refs update` moves. A version with no Version tag is never pinned silently.
_Avoid_: Release tag, version Ref

**Used version**:
The version of a package that a project's Package lockfile says the project uses. For a Python package only a version from PyPI's own index counts. It picks a Ref, through its Version tag, when a Repo is added from a registry or moved by `refs update`.
_Avoid_: Installed version, locked version, Ref

### Pinning

**Lock**:
The commit each Repo's Ref resolved to, committed with the project so everyone reads the same content. For a Repo that follows `HEAD` it also records the remote's default branch, for display.
_Avoid_: Snapshot, pin file

**Pin**:
What a Repo's Ref resolved to, as recorded in the Lock: the commit and, for a Ref that follows `HEAD`, the remote's default branch. The Lock holds one Pin per active Repo.
_Avoid_: Lock entry (an Entry is the Managed block's record), resolution

**Stale**:
A Lock whose Pins no longer match the active Repos: a Repo added or removed, or one whose url or Ref changed. Paths are not in the Lock, so changing them (or reordering them) never makes it stale; the Checkout follows Paths on the next sync. Stale does not mean re-resolve: a removed Repo only drops its entry.
_Avoid_: Outdated, invalid

**Checkout**:
The readable copy of a Repo's Paths at its locked commit, placed in the project's references directory.
_Avoid_: Clone, mirror, vendor

**Cache**:
The machine-wide store of the commits fetched for Repos that Checkouts are made from.
_Avoid_: Store, registry

**Outline**:
What `add` looks at before it asks anything: the Pin a Ref resolves to and every path in that commit's tree, read through the Cache without any file contents. A bad URL or Ref fails here, before anything is edited.
_Avoid_: Listing, index

**Break**:
What a new Ref takes from a Repo, found in the Outline of the new Ref before anything is written: a `path` that is no directory in it, or a `start` file that is not in it. A `packages` name the Manifests lack is not one; it is a warning. A Repo that breaks asks `Update?` before it moves.
_Avoid_: Conflict, invalid

### Telling the agent

**Agent file**:
A file the project designates to receive the Managed block, such as `AGENTS.md`. Only files refs manages are called this.
_Avoid_: Target, instructions file, rules file

**Managed block**:
The generated section of an Agent file listing the Entries of the active Repos. For now every Agent file receives the same block.
_Avoid_: Index, snippet, section

**Exclude rule**:
The line that keeps the references directory out of default search, kept in the repository's local git exclude file and never in a tracked ignore file. `refs init` and `refs sync` add it; outside a git worktree there is nowhere to put it.
_Avoid_: gitignore entry

**Tooling gap**:
A linter, formatter, type checker or editor (Zed) whose configuration does not exclude the references directory, so it would read the Checkouts: the Exclude rule does not reach tools that follow only their own config or `.gitignore`. refs only reports it, with the lines to add and how to silence it with `tooling_ignore`; it never edits the tool's files.
_Avoid_: Warning, missing ignore

**Preamble**:
The fixed instructions at the top of the Managed block. Owned by refs, not configurable, and versioned with the tool.
_Avoid_: Prompt, boilerplate, header

**Entry**:
One Repo's record in the Managed block: its id, Ref, short commit, description, Packages and Start.
_Avoid_: Header line, listing, item

### Keeping in sync

**Dangling checkout**:
A Checkout whose Cache entry is gone, for example because the Cache was wiped. It is a generated copy, so it is rebuilt, not refused.
_Avoid_: Broken, orphaned

**Foreign directory**:
A directory in the references directory that refs did not create. It is never deleted or overwritten.
_Avoid_: Conflict, unmanaged

**Checkout state**:
Where an active, locked Repo's Checkout stands against the Lock: in sync, absent, dangling, foreign, or stale (a different commit, or different Paths, with its dirty files if any). One classification, made once, that the Plan maps to actions and refusals. A Checkout that matches the Lock but is dirty is in sync.
_Avoid_: Status

**Dirty checkout**:
A Checkout with changes made by hand, including untracked files. It is not moved or removed unless forced.
_Avoid_: Modified, tainted

**Plan**:
The ordered changes a sync would make, decided from the Lock and what is on disk before anything is touched. A sync applies it; a check only reports it.
_Avoid_: Diff, dry run

**Out of date**:
A Project whose Plan is not empty apart from announcements: the Lock is stale or missing, a Checkout differs from the Lock, or an Agent file's block differs from the one that would be written. A refusal (a Foreign directory, a Dirty checkout, malformed markers) is not out of date: a sync would refuse it too.
_Avoid_: Drifted, unsynced
