# TODO

- Package lockfile lookup for `pypi:`: say that a package is only in the lockfile from another index, and a way to trust a PyPI mirror.
- A per-version registry lookup (dropped in #64): only if a package turns up that moved directories between versions. Its own ticket then.
- Re-pinning existing Repos to the Used version, and importing Repos from a project's dependency list. Both reuse `registry::used`.
- Tooling gaps: an unreadable or non-UTF8 config is a false miss and is untested; `.oxlintrc.jsonc` is not detected (check Oxlint's docs first); a symlinked Project directory silently loses the walk-up in `worktree::candidate_dirs`.
- Cleanups parked from review: `prompter` and `console` travel together through the `add` pipeline in `cli.rs`; `--ref` silently wins over a version; the `From<Gap>` copy into `Note`.
