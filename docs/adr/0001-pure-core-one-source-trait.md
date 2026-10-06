# Pure core, one `Source` trait, plan then apply

Config parsing, the active set, locking, Entry building, rendering, Agent-file splicing and planning are plain functions over data. The only trait is `Source`, which owns resolving a Ref and materialising Paths; `GitSource` is its only implementation. `sync` builds a plan of actions and either applies it or diffs it for `--check`. New features (other Sources, richer Entries, per-file placement, `--json`) then land in one module each, without touching the rest, and everything above `source` is tested with a fake `Source` and asserted on plans and text, not on git.

## Consequences

- The Lock entry carries a `source` tag (only `git` exists); only `source` code interprets the pin.
- `GitSource` keeps a few real-git contract tests, because those pin down facts about git that a fake would only restate (no fetch refspec in bare clones, per-worktree sparse config, blobless fetch). Pure parts of it (ref preference, URL normalisation, `ls-remote` parsing) are tested as data.
