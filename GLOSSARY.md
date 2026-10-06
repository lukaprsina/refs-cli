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
The names, as imported in code, of the packages a Repo documents or implements, in any language. Names are taken as written, with no ecosystem qualifier.
_Avoid_: Exports, libraries, modules

**Start**:
Files inside a Repo's Paths that are worth reading first.
_Avoid_: Entry points, hints

**Ref**:
A branch, tag or full commit SHA naming what to follow in a Repo.
_Avoid_: Version, revision

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
The machine-wide store of fetched Repo history that Checkouts are made from.
_Avoid_: Store, registry

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

**Preamble**:
The fixed instructions at the top of the Managed block. Owned by refs, not configurable, and versioned with the tool.
_Avoid_: Prompt, boilerplate, header

**Entry**:
One Repo's record in the Managed block: its id, Ref, short commit, description, Packages and Start.
_Avoid_: Header line, listing, item

### Keeping in sync

**Dangling checkout**:
A Checkout whose Cache history is gone, for example because the Cache was wiped. It is a generated copy, so it is rebuilt, not refused.
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
