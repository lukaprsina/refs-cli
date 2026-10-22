---
status: accepted
---

# `add` outlines the repo before it edits anything, and syncs once

`add` used to ask its questions blind, edit `refs.toml`, sync, read the new Checkout for `packages`, ask again, edit again and sync again. It now looks at the repo first, through a read-only part of `Source`, so the questions are asked against real data and everything is written by one edit and one sync. Amends ADR 0008 (what is asked and when) and ADR 0009 (the pipeline).

## Decision

- **Two new `Source` methods, keyed by URL** as `tags` is, since the Repo is not in the config yet:
  - `outline(url, ref) -> Outline { pin, paths }` resolves the ref, fetches the commit's trees into the Cache (ADR 0010) and lists every path from one `ls-tree -r --name-only`. A bad URL or ref fails here, with nothing edited.
  - `read(url, pin, paths) -> Vec<(path, text)>` fetches the named blobs in one batch and returns their text.
- **Policy is pure and in core.** Which paths are directories, which are Manifests to read and which files `start` may offer are functions over `Outline`, tested without an adapter. A Manifest or `start` file is one that will be in the Checkout: under the `paths`, at the root, or along the way to a path (the cone rule). Dot-directories and the skip list of ADR 0009 are skipped, below the chosen `paths`: a skipped directory the person chose (or one above it) hides nothing in it. At most 200 Manifests are read, shallowest first, and a status line says when more were left. `infer_packages` becomes a function over `(path, text)` pairs, and also returns the root Manifest's description.
- **Package filters** (npm only, as the other ecosystems have no equivalent): drop a name that is not a valid npm package name (this removes the subpath-export shim `package.json` files, such as `solid-js/h`), and `private: true`. A napi-rs platform package (`<package>/npm/<platform>/package.json` with a `package.json` above the `npm`) is dropped for every ecosystem: it holds a binary and is not imported by name. For Cargo also drop `publish = false`. A "no version" rule was rejected: it is wrong for Cargo, PyPI and Go, and misses redux-toolkit's shims, which have a valid scoped name and a version.
- **When `outline` runs.** For a plain URL, forge or registry add, with a `Source`, when there is a prompter or no `--packages` was given. A registry add is outlined too, at the tag it pinned, so a bad tag fails before any question and its `paths` and `start` are picked from the real tree. The registry still decides the starting values: the ref, the `paths` (ticked in the picker, and kept by Enter) and the one `packages` it named; the inferred list never replaces that. Its Manifests are read only when `packages` or the description is missing. With no one to ask, a registry add has its packages and does not outline. `--no-sync` and `--offline` have no `outline`: the questions are free text and nothing is inferred. `--packages` with `--no-input` skips it too. Only `prepare` holds the `Option<Outline>`.
- **One edit, one sync.** `add::run` is `prepare` (which now includes `packages`), one `Edit::Add` and one sync. The `SetPackages` edit, `sync_and_infer`, `Skipped` and the second report go. Sync 1's `verify` stays, for adds that never had an `outline`.
- **The ref is asked before `outline`**, and the rest after. The sequence is URL, ref, `outline`, then the summary below. Esc at any point edits nothing.
- **A summary, then one gate.** `add` shows the values it has (id, ref, paths, start, packages, description, group) and asks `Customize?` (default no). Enter adds as shown; yes asks, in order, id, group, description, `paths`, `start`, `packages`, each prefilled with the summary's value, so Enter through all of them gives the same Repo as no. A prefilled text uses `inquire`'s initial value, not its default, so a person can clear it. `packages` and the `start` candidates are recomputed from the Manifests already read when `paths` changes. A value with no default (the URL; a description none was found for) is asked before the summary, and may be left empty. A taken id is asked with a validator, never suffixed. A registry or forge add goes through the same summary and gate.
- **The pickers.** `paths` and `start` are flat, fuzzy `inquire::MultiSelect` lists behind one new `pick` method on `Prompter`. An empty selection means the whole repo (`paths`) or none (`start`), said in the message. A child of a selected directory is dropped (cone mode includes it). Directories are listed in path order, capped at 5,000, shallowest first, with a status line pointing at `--paths` past the cap. `start` offers only docs-like files in the Checkout: `.md`, `.mdx`, `.markdown`, `.rst`, `.adoc` and `README*`, with the same cap; anything else is `--start`. A repo with no directories skips the question. Without a terminal or an `Outline` the free-text questions stay.
- **`packages` is a picker too**, with every inferred package ticked to begin with (`pick` takes the options to preselect), so Enter keeps them all, a long list does not wrap, and a package is dropped by unticking it. A package nobody inferred is named with `--packages`.
- **The summary is shown even with `-q`** (`Say::shown`): the `Customize?` question is about it.
- **Group** is one `suggest` prompt in the customize path, empty for none. The summary shows `group (none)`.
- **Description** is prefilled, and still shown: from the registry's own description (it describes the package named, not the workspace), otherwise from the root Manifest, otherwise asked. It goes into `AGENTS.md`, so it is never written unseen.
- The equivalent command is printed before the sync, whenever there was someone to ask, so a failed sync does not lose it.

## Considered and rejected

- **Sync 1 before the questions.** It edits `refs.toml` and breaks "a cancel edits nothing", and it checks out the whole repo before `paths` is known.
- **Keeping two syncs, with `outline` for validation only.** Less to touch, but it keeps sync 2, `SetPackages` and the duplicated output.
- **Descriptions from a forge API.** One adapter per host, rate limits, a new failure mode in a command that needs only git, and the repo's "About" text is weak for a monorepo.
- **A custom picker (`nucleo-picker`).** About ten new crates and an alternate screen. Revisit only if `MultiSelect` proves too slow or plain.
- **Pre-picked `paths`.** Pressing Enter would write a sparse checkout the person did not choose.
- **npm dist-tags (`npm:solid@next`).** npm-only; `npm:solid --ref next` already works, and the `bad_version` message says so.
- **A cap and a count on `packages`.** Not needed once the shims are filtered; add one if a real repo shows a need.

## Consequences

- `Source` gains two methods (ADR 0001), and `GitSource` the `ls-tree` and `cat-file` code they need. The `fake` adapter serves a canned `Outline` and blobs.
- `packages` may come from a different commit than the Checkout: sync 1 resolves the ref again, and a push in between makes them differ. The Lock stays truthful and the person can fix `packages`.
- `--no-sync` stays, with a tripwire: if `Option<&dyn Source>` or `Option<&Outline>` ends up in more than two function signatures, remove it from `add`.
- TODO: try the 200-Manifest and 5,000-directory caps on a very large repo (chromium).
- TODO: tune the picker's page size (15 rows now) on a real terminal.
