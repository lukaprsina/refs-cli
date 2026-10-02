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

**Preamble**:
The fixed instructions at the top of the Managed block. Owned by refs, not configurable, and versioned with the tool.
_Avoid_: Prompt, boilerplate, header

**Entry**:
One Repo's record in the Managed block: its id, Ref, short commit, description, Packages and Start.
_Avoid_: Header line, listing, item
