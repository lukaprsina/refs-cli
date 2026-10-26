# TODO

- Package lockfile lookup for `pypi:`: say that a package is only in the lockfile from another index, and a way to trust a PyPI mirror.
- A per-version registry lookup (dropped in #64): only if a package turns up that moved directories between versions. Its own ticket then.
- Re-pinning existing Repos to the Used version, and importing Repos from a project's dependency list. Both reuse `registry::used`.
- Tooling gaps: an unreadable or non-UTF8 config is a false miss and is untested; `.oxlintrc.jsonc` is not detected (check Oxlint's docs first); a symlinked Project directory silently loses the walk-up in `worktree::candidate_dirs`.
- Cleanups considered and dropped: bundling `prompter` and `console` in `cli.rs`. `Asking` already holds the prompter, each use falls back differently without one, and `console` is used everywhere, so a bundle would only shorten parameter lists.
- Advice strings (the `--ref` pointer in `add/pin.rs`, `add/expand.rs` and `update/assess.rs`): considered and dropped. They share only a clause, and the six messages differ in what they say, so a helper would change wording for little.
- Left as they are: `commands::init` next to the domain `init` (different module trees), and the exit code in `commands/self_update.rs` (one line, no report to sit beside).
