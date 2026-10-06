# The CLI follows uv's verbs and flags

`refs` copies `uv`'s model (config is intent, the lock is resolution, `sync` makes the disk match) and, where a command or flag has a uv counterpart, its name and meaning: `lock`/`sync`, `--no-sync`, `--check`, `--locked`, `--upgrade`, `--upgrade-package`, `--project`, `--offline`, `-q`/`-v`. We invent our own only where the domain has no uv equivalent. Users who know uv should guess `refs` right, and an agent that has seen uv's help text gets ours for free.

Known divergences, kept on purpose: `enable`/`disable` and Groups (uv has neither; Repos and Groups are not colocated in the TOML, so a switch earns its place), and `add` metadata flags (`--paths`, `--packages`, `--start`, `--description`), which override what `add` infers (spec §3.2, rank 1), `add` prompting for what it was not given on a terminal (ADR 0008; uv never prompts), and `update`, which uv spells `uv self update`: `refs` has no other `self` commands to group it with, so `refs update` is the short form, and `--check` exits `3` as `sync --check` does.

Where the CLI still differs from uv by accident, not by design, it is a bug to fix, not a precedent: see the alignment batch in spec §3.2.

## Output and exit codes

Like uv, a command that changes something says so in short status lines on stderr, one consistent style (lowercase, verb and object, no full stop): `refs.toml`, each Group created, the Lock, each Checkout created, moved or removed, each Agent file updated (in that order: a Group is part of the `refs.toml` edit). A command that changed nothing says so in one line, an in-sync `sync` included. `-q` silences status lines, problems are always printed, and stdout carries data only. "Run `refs sync`" is printed only when the Project is left incomplete (a failed sync, or an edit with `--no-sync`), and like a problem it survives `-q`, never as an unconditional reminder. Which hint a run gets follows from where it stopped, so `sync` sets it on the `Report` of the run at that point; `cli` only gives each its words.

Exit codes are `0`, `1` and `2` as in uv, plus `3` for `sync --check` finding the Project out of date. That `3` is a recorded divergence from uv, which exits 1 for a failed `--check`: CI and pre-commit hooks need to tell "out of date" from "refused or broke", so refusals and errors keep `1`.
