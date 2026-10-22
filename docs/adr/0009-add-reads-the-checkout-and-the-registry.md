---
status: accepted
---

# `add` learns `packages` from the registry or from the repo's Manifests

`packages` is the field a person least wants to type and `add` can mostly work out. There are two sources, depending on how the Repo was named. Amends ADR 0008, whose single "paths, packages and start" confirm no longer fits. ADR 0011 changed when the Manifests are read (before the edit, not from the Checkout); what is read and how it is filtered is below.

## Decision

- **Registry shorthand.** `add` accepts `npm:<name>`, `cargo:<name>` and `pypi:<name>` (explicit prefix, never a bare name, as with `gh:` and `gl:`, which `registry::forge` expands without the registry). It asks the registry for the package's repository and fills `url`, `paths` (npm's `repository.directory`) and `packages` (the name given). It is input sugar: `refs.toml` stores the expanded git URL, so the Lock, the Source and every command after `add` never see the prefix. A shorthand may name a version (`npm:<name>@<version>`); `add` maps it to a Version tag and pins that as the Ref (spec §7.2). With no version, `add` reads the Used version from the project's Package lockfile (spec §7.2); with none found, the shorthand follows `HEAD`, and the registry's latest version is never used as a tag. A version with no tag is never pinned silently: `add` asks, or warns and follows `HEAD`. A version together with `--ref` is refused, as the two pin differently and may disagree.
- **Inference from the Manifests.** For any other `add`, `packages` is read from the Manifests the Checkout would hold (ADR 0011): `package.json` `name`, `Cargo.toml` `[package].name`, `pyproject.toml` `[project].name`, `go.mod` `module`. Every Manifest in the Paths, at the root and along the way is read, so a workspace needs no member expansion. It skips dot-directories and directories that hold dependencies, build output, examples or test fixtures (`node_modules`, `target`, `vendor`, `dist`, `examples`, `fixtures`, `test`, `tests`), npm Manifests marked `"private": true` or whose name is not a valid npm package name (the subpath-export shims), Cargo packages with `publish = false`, and Manifests without a name. Names are taken as written, deduplicated and sorted; a miss is empty, not an error. It is a pure function over the Manifests' paths and text.
- **Fill, don't replace.** An explicit `--paths` or `--packages` wins over what the registry says, and an explicit `--packages` skips inference. Inferred names are only the default of the `packages` prompt. A registry add is different: the name the person typed is the package, so the registry's `packages` is written as it is, with no question about it, and its `paths` is where the `paths` picker starts (ADR 0011). A name that is wrong for a monorepo is fixed in `refs.toml`.
- **The pipeline** is in ADR 0011: the Manifests are read before the edit, `packages` is part of the summary the person confirms, and one edit and one sync write it. Without a terminal the inferred names are written. With `--no-sync` or `--offline` nothing is inferred.
- **No second kind of Source.** A registry package is resolved to a git Repo and pinned like any other. We do not fetch published tarballs: the Lock, Ref, Pin and Cache are all git, and a tarball has no history to pin.

## Why

- A Repo's manifests state the package names exactly. Reading them costs one batched blob fetch, with no resolver and no installed tool.
- Keeping the prefix as input sugar means a future non-git Source can take `npm:` for itself without changing what is stored. The reverse (a stored `npm:` entry) would put the registry's answer into `refs.toml`, which changes as maintainers move repositories.
- For a registry add the name is already known, so inference is only needed for plain URL adds. We still infer for those because that is the common case.

## Consequences

- `Packages` in the glossary is no longer "as imported in code". A crate `foo-bar` imports as `foo_bar`, and `Pillow` imports as `PIL`; inference gives the manifest name and the person can correct it. `Manifest` becomes a glossary term.
- A sparse Checkout only has the Manifests under its Paths and, in cone mode, those at the root and along the way to each path. Inference reads those, so `paths = ["docs"]` still finds a root `package.json`, but not the members of a monorepo. A miss is normal, not a failure.
- The registry lookup needs an HTTP client. `reqwest` is already in the tree through `axoupdater`; enabling `blocking`, `rustls` and `json` (with default features off) adds two tiny crates and no measurable binary growth.
- Linter/formatter coverage is not decided here: a Tooling gap is reported by `sync` and edits nothing (spec §7.6).
