# The CLI follows uv's verbs and flags

`refs` copies `uv`'s model (config is intent, the lock is resolution, `sync` makes the disk match) and, where a command or flag has a uv counterpart, its name and meaning: `lock`/`sync`, `--no-sync`, `--check`, `--locked`, `--upgrade`, `--upgrade-package`, `--project`, `--offline`, `-q`/`-v`. We invent our own only where the domain has no uv equivalent. Users who know uv should guess `refs` right, and an agent that has seen uv's help text gets ours for free.

Known divergences, kept on purpose: `enable`/`disable` and Groups (uv has neither; Repos and Groups are not colocated in the TOML, so a switch earns its place), and `add` metadata flags (`--paths`, `--packages`, `--start`, `--description`), which override what `add` infers (spec §3.2, rank 1).

Where the CLI still differs from uv by accident, not by design, it is a bug to fix, not a precedent: see the alignment batch in spec §3.2.
