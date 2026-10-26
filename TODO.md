# TODO

- Package lockfile lookup for `pypi:`: say that a package is only in the lockfile from another index, and a way to trust a PyPI mirror.
- A per-version registry lookup (dropped in #64): only if a package turns up that moved directories between versions. Its own ticket then.
- Re-pinning existing Repos to the Used version, and importing Repos from a project's dependency list. Both reuse `registry::used`.
- Tooling gaps: an unreadable or non-UTF8 config is a false miss and is untested; `.oxlintrc.jsonc` is not detected (check Oxlint's docs first); a symlinked Project directory silently loses the walk-up in `worktree::candidate_dirs`.
- Cleanups considered and dropped: bundling `prompter` and `console` in `cli.rs`. `Asking` already holds the prompter, each use falls back differently without one, and `console` is used everywhere, so a bundle would only shorten parameter lists.

## repo structure rewrite

- After the `ui`/`commands` move (#92-#95): split the long "Where decisions live" paragraph in `CODING_STANDARDS.md` into one bullet per command.
- After the move: a `Project { root, config }` value for what `load`, `with_source` and every command pass together; a context value for `add::settle_ref` and `add::look` (six parameters each); and `registry_add` is worked out from `requested` in two places in `add`.
- After the move: the advice strings ("use `--ref` to move it", `refs update {id} --ref <tag>`, the `pin` hint) in one helper built around what they share (the command form, the `--ref` pointer), not a constants file of whole sentences. Natural home: `ui`'s report words.
- Name overlaps to settle after the move: `commands::init` and the domain `init`; `list_status` in `cli` and in `list`; `cli::Terminal` (styles) and the `inquire` `Terminal`; `tests/registry_pin.rs` keeps its old name.
