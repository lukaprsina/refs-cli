# TODO

## Known gaps

- [ ] `list` dimming has no test through the binary (a pipe is never a terminal); only `disabled_lines_are_dimmed_only_with_color` covers it.
- [ ] Real terminal detection in `cli::run` is untested; only the `Terminal` seam is.
- [ ] If every active Repo fails to lock after a `remove`/`disable`, the Managed block is left as is rather than stripped.
