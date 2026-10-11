# Package lockfiles for `pypi:` and `bun.lock`

Research for the first `TODO.md` item: reading `uv.lock`, `poetry.lock` and `bun.lock` for the Used version. Dates: lockfiles were generated on 2026-10-11 with Poetry 2.5.1 (`uv tool install poetry`) and the uv on this machine. `bun.lock` was read from real files on this machine, not generated. I have not read any format's own specification; everything below is observed from those files, so check a claim against the tool's docs before relying on it for an edge case I did not hit.

## 1. Where it plugs in

`src/registry/used.rs`: a `Format` enum, `Format::of(ecosystem)` (file name and format, in preference order), and one module per format with `used(name, text)`. Before this change `reads(Ecosystem::Pypi)` was false because `Format::of(Pypi)` was empty, and `cli::used_version` returned early on it. Listing formats for Pypi turned the lookup on, with the existing messages for "no lockfile", "not in the lockfile" and "several versions"; `reads` was then true for every ecosystem and was removed.

## 2. `uv.lock`

TOML: `version = 1`, `revision`, `requires-python`, then `[[package]]` tables with `name`, `version`, `source`, `dependencies`.

- **Source** is an inline table. Seen: `{ registry = "https://pypi.org/simple" }`, `{ registry = "https://download.pytorch.org/whl/cpu" }` (a custom index), `{ git = "https://…?tag=v2.0.0#<sha>" }`, `{ directory = "lib" }` (local path dependency), `{ virtual = "." }` and `{ editable = "." }` (the project itself).
- **Direct**: the project's package (source `virtual` or `editable`) lists what it asks for in `dependencies`, `[package.dev-dependencies]` (a table of group name to dependency list) and `[package.optional-dependencies]`. Checked on a generated workspace lock (2026-10-11): `[package.optional-dependencies]` is a table of extra name to dependency list, like dev groups.
- **Several versions of one package** are separate `[[package]]` entries, each with `resolution-markers`. The project's dependency entry then also names the version and source: `{ name = "numpy", version = "2.4.6", source = { registry = "…" }, marker = "…" }`. Without several versions it is just `{ name = "click" }`. This is the same shape as `Cargo.lock`'s `name` or `name version`.
- **Names** are normalized (lowercase, `-`). A user may type `pypi:Pillow`; compare names by PEP 503 normalization (lowercase, runs of `-`, `_`, `.` become `-`).
- The root's `version` can be absent when it is dynamic (from memory, not seen), so make it optional.
- A workspace's members are `source = { editable = "member" }` next to the root's `virtual`, and `[manifest] members` lists them; the member's dependencies count as the project's. A path dependency outside the workspace is `{ directory = … }`, so not mistaken for a member. Fixture: `tests/fixtures/used/uv-workspace.lock`.
- Fixtures on this machine: `docs/private/refactor/vendor/agentskills/skills-ref/uv.lock` (an editable root with dev dependencies, one PyPI index), and the generated one in section 4.

## 3. `poetry.lock`

TOML: `[[package]]` with `name`, `version`, `description`, `optional`, `python-versions`, `groups` (Poetry 2), optional `markers`, then `[package.dependencies]`, `[package.extras]`, `files`; a `[metadata]` table with `lock-version = "2.1"`, `python-versions` and `content-hash`.

- **Source**: absent for a package from PyPI. Seen: `[package.source] type = "git"` with `url`, `reference`, `resolved_reference` (plus `develop = false` on the package); `type = "legacy"` with `url` and `reference` (the source's name) for a package from a custom index.
- **No root entry and no direct information.** `groups = ["main"]` also holds for transitive packages, so it says which group pulled a package in, not whether the project asked for it. Reading `pyproject.toml` would be the only way; leave every version `direct: false`, as yarn does.
- Names are normalized (`Pillow` in `pyproject.toml` is `pillow` in the lock).
- I generated this with Poetry 2.5.1; older Poetry (1.x) locks have no `groups` and a different `lock-version`. Not tested.

## 4. Private indexes

Both formats say where a version came from, so a private index is detectable.

- uv: `source.registry` is a URL; `https://pypi.org/simple` is PyPI.
- Poetry: no `[package.source]` is PyPI; `type = "legacy"` is another index.

`pypi:<name>` asks pypi.org for the repository and then looks for a tag of the Used version in that repository. A version from another index may be a mirror of the same project (PyTorch's index serves `numpy`) or an unrelated internal package with the same name; refs cannot tell them apart, and a version that happens to match a tag would pin the wrong history. `Cargo.lock` reads any registry (`registry+` or `sparse+`); I recommend not copying that here.

Recommendation: only PyPI counts as a registry version (uv: `registry` is `https://pypi.org/simple`, optionally with a trailing slash; Poetry: no source). Everything else is skipped like git and path sources. The known cost: a PyPI mirror (Artifactory and similar) is skipped, and the add ends with "not in the lockfile; follows the default branch". Two refinements, neither needed first: say in the message that the package is only in the lockfile from another index, and a setting to trust an index.

The generated lockfiles (uv with a custom index, a git dependency, a path dependency, a dev group and two numpy versions split by Python version; Poetry with git and legacy sources) are trimmed into `tests/fixtures/used/uv.lock` and `poetry.lock`.

## 5. `bun.lock`

Text lockfile (bun 1.2 and later). It is JSONC: trailing commas and (by the format) comments, so `serde_json` rejects it. `bun.lockb` is binary and never read.

- Top level: `lockfileVersion`, `configVersion`, `workspaces`, `packages`. Seen in `docs/private/refactor/vendor/executor/bun.lock` (4898 lines).
- **`workspaces`**: keys are paths (`""` is the root, `"apps/cli"`), each with `name`, `dependencies`, `devDependencies` and so on. That is the direct information: direct when some workspace's dependency groups name the alias.
- **`packages`**: `"<key>": ["<name>@<version>", "<registry>", { dependencies… }, "<integrity>"]`. The key is the install path: `"@ant-design/icons"` at the top, `"agents/yaml"` or `"@astrojs/compiler-binding-wasm32-wasi/@napi-rs/wasm-runtime"` when nested. The name is in element 0, so an alias is found by its real name, as in `package-lock.json`.
- Not registry versions: `"@executor-js/api@workspace:packages/core/api"` (a workspace member; element 0 has a `workspace:` spec). The existing `used/yaml.rs::is_registry_version` already rejects specs with `:` or `@`, and `split_pkg_spec` handles scoped names, so both can be reused. I did not see `github:` or `file:` entries in this file; from memory they appear the same way in element 0.
- **JSONC**: no JSONC crate is in the tree. A string-aware pass that drops trailing commas and comments is about 30 lines; the alternative is a new dependency (`jsonc-parser` and similar, not evaluated). Recommend the small pass, tested against this file.
- Order in `Format::of(Npm)`: after the three existing ones is enough; a project has one lockfile and `add` already says which others were left unread.

## 6. Work

Done: the Python change (section 6's first paragraph), in `used/uv.rs`, `used/poetry.rs` and `used/pypi.rs`, fixtures `tests/fixtures/used/uv.lock` and `poetry.lock`. `Format::of(Pypi)` is non-empty, so the `reads` gate was removed. Then `bun.lock` (`used/bun.rs`, fixture `tests/fixtures/used/bun.lock`, generated with bun 1.3.11: `lockfileVersion` 2, a github dependency, an alias, a workspace member and `ms` at three versions; the 4,898-line v1 file from section 5 was also read). Only trailing commas are handled, not comments: bun writes none. A copy nested under a workspace is keyed `<workspace name>/<alias>`, which is how a nested version is told to be direct.

Python change (turns `pypi:` on): `Format::{UvLock, PoetryLock}`, `used/uv.rs`, `used/poetry.rs`, a shared name comparison, `Format::of(Pypi)`, the fixtures, the changed `pypi:` test and a new pin test, then docs (spec §7.2 and the lines that list lockfiles, ADR 0009, README `add` row, glossary, `TODO.md`, the comments in `used.rs`). Bun change: `Format::BunLock`, `used/bun.rs` with the JSONC pass, a fixture trimmed from the real file, a find-order test, the same docs.

Open: how `pypi:` tag lookup fares on Python repositories is not measured; many tag `v1.2.3`, which `tag_for` already tries, but monorepos and unusual schemes will hit "version has no tag" (ask, or warn and follow `HEAD`).
