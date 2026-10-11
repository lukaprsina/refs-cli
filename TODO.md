# TODO

1. Remove `doctor` from the spec (§3.2, §10.1, the roadmap, and the `doctor` mentions at §3, §6, §7 and §9). #5 is `wontfix`; its checks fold into `sync` as advisories. Also `docs/adr/0002` and `0006`.
2. Later formats and ecosystems for the Package lockfile lookup: `bun.lock`, `uv.lock`, `poetry.lock`, and `pypi:` (it follows `HEAD` today). Never binary `bun.lockb`.
3. A per-version registry lookup (dropped in #64): only if a package turns up that moved directories between versions. Its own ticket then.
4. Re-pinning existing Repos to the Used version, and importing Repos from a project's dependency list. Both reuse `registry::used`.
5. Tooling gap gaps: an unreadable or non-UTF8 config is a false miss and is untested; `.oxlintrc.jsonc` is not detected (check Oxlint's docs first); a symlinked Project directory silently loses the walk-up in `worktree::candidate_dirs`.
6. Cleanups parked from review: `Release` is not a glossary "release" (rename); `used_version` in `cli.rs` is about 70 lines; `source: Option<&dyn Source>` stands for the `--no-sync` mode; `Data Clumps` in `fill_add`/`complete_add` (`packages`, `ref_settled`, `prompter`) and `Registry` errors (`ecosystem`, `name`); `--ref` silently wins over a version; the `From<Gap>` copy into `Note`.
