---
status: accepted
---

# `add` learns `packages` from the registry or from the new Checkout

`packages` is the field a person least wants to type and `add` can mostly work out. There are two sources, depending on how the Repo was named. Amends ADR 0008, whose single "paths, packages and start" confirm no longer fits: `packages` can only be inferred after the Checkout exists.

## Decision

- **Registry shorthand.** `add` accepts `npm:<name>`, `cargo:<name>` and `pypi:<name>` (explicit prefix, never a bare name, as with `gh:`). It asks the registry for the package's repository and fills `url`, `paths` (npm's `repository.directory`) and `packages` (the name given). It is input sugar: `refs.toml` stores the expanded git URL, so the Lock, the Source and every command after `add` never see the prefix. A shorthand may name a version (`npm:<name>@<version>`); `add` maps it to a Version tag and pins that as the Ref (spec §7.2). A shorthand with no version follows `HEAD`, and does not use the registry's latest version as a tag. A version with no tag is never pinned silently: `add` asks, or warns and follows `HEAD`. Reading the version from a project's Package lockfiles is decided separately.
- **Inference from the Checkout.** For any other `add`, once the sync has made the Checkout, `packages` is read from the Manifests in the Checkout: `package.json` `name`, `Cargo.toml` `[package].name`, `pyproject.toml` `[project].name`, `go.mod` `module`. It walks the whole Checkout, which is only the Paths plus the root files a sparse Checkout keeps, and reads every Manifest it finds, so a workspace needs no member expansion. It skips dot-directories and directories that hold dependencies, build output, examples or test fixtures (`node_modules`, `target`, `vendor`, `dist`, `examples`, `fixtures`, `test`, `tests`), npm Manifests marked `"private": true`, and Manifests without a name. Names are taken as written, deduplicated and sorted; a miss is empty, not an error. It is a pure function over a directory, so it works for any Source and needs no new `Source` method.
- **Fill, don't replace.** An explicit `--paths` or `--packages` wins over what the registry says, and an explicit `--packages` skips inference. Inferred names are only the default of the `packages` prompt. A registry add is different: the name the person typed is the package, so the registry's `paths` and `packages` are written as they are, with no question about them. That keeps a registry add to one sync. A name that is wrong for a monorepo is fixed in `refs.toml`.
- **The pipeline.** Prompts, `edit::add`, lock and sync, infer, confirm `packages` (default: the inferred names; free text if none were found), a second edit that sets `packages`, then a second sync that only re-renders the Managed block. Without a terminal the confirm is skipped and the inferred names are written. With `--no-sync` there is no Checkout, so nothing is inferred. Declining or cancelling at the confirm leaves the Repo added without `packages` and says so.
- **No second kind of Source.** A registry package is resolved to a git Repo and pinned like any other. We do not fetch published tarballs: the Lock, Ref, Pin and Cache are all git, and a tarball has no history to pin.

## Why

- A Repo's manifests state the package names exactly, and `add` already runs a sync that puts the Checkout on disk. Reading it costs a directory walk, with no resolver and no installed tool.
- Keeping the prefix as input sugar means a future non-git Source can take `npm:` for itself without changing what is stored. The reverse (a stored `npm:` entry) would put the registry's answer into `refs.toml`, which changes as maintainers move repositories.
- For a registry add the name is already known, so inference is only needed for plain URL adds. We still infer for those because that is the common case.

## Consequences

- `Packages` in the glossary is no longer "as imported in code". A crate `foo-bar` imports as `foo_bar`, and `Pillow` imports as `PIL`; inference gives the manifest name and the person can correct it. `Manifest` becomes a glossary term.
- A sparse Checkout only has the Manifests under its Paths and, in cone mode, those at the root and along the way to each path. Inference reads whatever is there, so `paths = ["docs"]` still finds a root `package.json`, but not the members of a monorepo. A miss is normal, not a failure.
- `edit` gains a way to set `packages` on an existing Repo, cut by text as in ADR 0007.
- The registry lookup needs an HTTP client. `reqwest` is already in the tree through `axoupdater`; enabling `blocking`, `rustls` and `json` (with default features off) adds two tiny crates and no measurable binary growth.
- Version detection from a project's lockfiles (to pick a Ref), tag mapping, and linter/formatter coverage are not decided here.
