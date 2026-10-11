# TODO

- Later formats and ecosystems for the Package lockfile lookup: `bun.lock`, `uv.lock`, `poetry.lock`, and `pypi:` (it follows `HEAD` today). Never binary `bun.lockb`.
- A per-version registry lookup (dropped in #64): only if a package turns up that moved directories between versions. Its own ticket then.
- Re-pinning existing Repos to the Used version, and importing Repos from a project's dependency list. Both reuse `registry::used`.
- Tooling gaps: an unreadable or non-UTF8 config is a false miss and is untested; `.oxlintrc.jsonc` is not detected (check Oxlint's docs first); a symlinked Project directory silently loses the walk-up in `worktree::candidate_dirs`.
- Cleanups parked from review: `Release` is not a glossary "release" (rename); `used_version` in `cli.rs` is about 70 lines; `source: Option<&dyn Source>` stands for the `--no-sync` mode; `Data Clumps` in `fill_add`/`complete_add` (`packages`, `ref_settled`, `prompter`) and `Registry` errors (`ecosystem`, `name`); `--ref` silently wins over a version; the `From<Gap>` copy into `Note`.
