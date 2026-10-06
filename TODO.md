# TODO

## Known gaps

- [ ] `list` dimming has no test through the binary (a pipe is never a terminal); only `disabled_lines_are_dimmed_only_with_color` covers it.
- [ ] Real terminal detection in `cli::run` is untested; only the `Terminal` seam is.
- [ ] A `start` that names a directory directly under the repo root passes `verify` (it exists at the commit) but is not in the Checkout unless a `paths` entry covers it; spec §4 says a root `start` must be a file. `verify` could require `EntryKind::Blob` for a `start` with no `/` when `paths` is set.
- [ ] No test for a failed `refs.toml` write after a Lock prune (the Lock is already pruned, the edit is `Rejected`). Needs a portable way to make `atomic::write` fail.
- [ ] A failed Lock write in `sync::edit` is reported as `Blocked`, so it hints at `--no-sync`, which would not help. Wants a result type for the run that tells these apart (architecture review candidate 2).
