---
status: proposed
---

# A registry add keeps its Origin in `refs.toml`, and `update` moves pins from it

`npm:solid-js@2.0.0-rc.17` stored only the git URL and the tag, so nothing could later say which Package lockfile entry the pin followed, and a pinned Repo could only be moved by `remove` then `add` or by hand. A Repo added from a Registry now also stores `origin = "npm:solid-js"` (a **Package**, never a version: the version is the Ref). `refs update` reads it to find the Used version and its Version tag again.

## Decision

- **`url` stays the git URL.** The Lock compares the config's `url` and `ref` with its own (`lock::Field`), and `sync --check` depends on that. A shorthand in `url` would make `sync` ask the Registry, break offline use and tie the result to the Registry's current `repository`.
- **A separate field, written for every Registry add**, never for a Forge shorthand (its expansion is a pure string). Presence does not depend on how the Ref was chosen. It is validated when the config loads: `npm:`, `cargo:` or `pypi:` and a name, no version.
- **Only `add` and `update` read it.** `update` never asks the Registry: the Origin gives the ecosystem and name for `used::answer` and `tag_for`, the URL stays as written, and the tags come from `Source::tags(url)`.
- **`refs update [id…]`** (bare, in a terminal: a multi-select of every Repo as `current → new`, nothing preselected). A floating Ref moves in the Lock, a pinned one moves to the Version tag of the lockfile's version, and then it syncs. When no lockfile answers (no Origin, no lockfile, or the Package is not in it) a pinned Repo moves to the newest tag of its own kind: the highest semver version among the tags with an equal prefix (so sibling Packages of a monorepo are never chosen), ignoring prereleases for a stable tag. A tag with no version stays, with a note pointing at `--ref`. A major bump is allowed: the `old → new` label and the pick are the consent (#88). Picking or naming a Repo is the consent: a clean move has no question, and a Repo that loses a path or `start` file asks `Update?` and re-asks only the broken fields, with the still valid entries ticked. `packages` that vanished from the Manifests are kept and warned about. `description` is never touched. `--list` is a dry run that also works without a terminal, and without a terminal only `update <id> --ref <tag>` moves anything. One edit and one sync; Esc edits nothing.
- **The `paths` picker hides dot-directories** (`.github`, `.changeset`) and what is below them, by the dot rule the Manifest and `start` listings use. The rest of their skip list (`examples`, `tests`, ...) stays offered: those are plausible `paths`. `--paths .github` still works.
- ~~**`refs lock --upgrade` stays**~~ (superseded by ADR 0013: `lock` is gone; `update` moves floating Refs).
- **The Ref is asked on a Registry add only when nothing was pinned.** A pin from a version or the lockfile prints a hint instead (`use `--ref` to track a branch or choose another tag`), on every path that settled the Ref without asking. `npm:solid-js@next` is refused with help that says `next` looks like a tag and shows `npm:solid-js --ref next` as the form for a git ref named `next`, which may not be the dist-tag.
- **`refs update` (self-update) becomes `refs self update`**, with no alias, so an old script never starts moving pins.

## Considered and rejected

- The shorthand in `url`: see above.
- `add` replacing a taken id: it would overwrite a tuned `description`, `start` and `group` (ADR 0011 refuses a taken id).
- Dropping pinned Registry versions: the lockfile-to-tag path is the only add with no question, and the pin is what makes a Reference match the version the project runs.
- Writing the Origin only when the Ref came from the lockfile: its presence would then depend on how the Ref was chosen.
- Moving pins in `sync`: `sync` is locked.
- `update --all` and a moving `--no-input`: bare `update` is the picker, and a lockfile-driven move without a person is the case that is too easy to get wrong.

## Consequences

- Amends ADR 0009 ("the prefix and version are never stored": the prefix is now kept as the Origin, the version still is not) and the glossary's **Registry**.
- `Repo` is `deny_unknown_fields`: `origin` is a config-format change, and a Repo added before it has none until it is hand-edited in or given `add --origin`, which records one on any add (and replaces a shorthand's) without reading a lockfile (#89).
- Shipped with #81: `refs update <id…> [--ref] [--force]` (the lockfile and branch moves, the dirty skip, one edit and one sync), `Edit::Change` and the `refs self update` rename. Shipped with #82: the break rule (`listing::gone`, advisory; `verify` stays the check that must pass), `Update?` and the re-ask of the broken fields (`update::ask::fill_update`, sharing the `paths`, `start` and `packages` questions with `add` through `prompt::pickers`), and a deleted Ref asked again. Without a terminal a break skips only that Repo. Shipped with #83: the bare-`update` picker (`find` learns what each Repo comes to, from the remote and the lockfile alone; `settle` holds back, asks and outlines only for the Repos named or picked). Shipped with #84: `refs update --list`, the dry run (the same `find`, then `look` and the hold for each mover, nothing asked or written). The ADR stays `proposed` until those land.
