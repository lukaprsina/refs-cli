# Pure core, one `Source` trait, plan then apply

Config parsing, the active set, locking, Entry building, rendering, Agent-file splicing and planning are plain functions over data. The only trait over the project's data is `Source`, which owns resolving a Ref and materialising Paths; `GitSource` is its only implementation. The seams to the person and the program (`Prompter`, ADR 0008; `Updater`, for `refs update`) are not part of the core: `cli` takes them as arguments, and tests script them. `sync` builds a plan of actions and either applies it or diffs it for `--check`. New features (other Sources, richer Entries, per-file placement, `--json`) then land in one module each, without touching the rest, and everything above `source` is tested with a fake `Source` and asserted on plans and text, not on git.

## Consequences

- `add` asks the `Source` about a remote that is not in the config yet, by URL: its tags (to turn a version into a Version tag), and its tree and Manifests (`outline` and `read`, ADR 0011). It takes the `Source` as an `Option`: `--no-sync` has none, and so asks nothing. This is not a second kind of seam to the project's data.

- Only `source` code interprets the pin.
- `GitSource` keeps a few real-git contract tests, because those pin down facts about git that a fake would only restate (per-worktree sparse config, blobless fetch, a Cache with no history). Pure parts of it (ref preference, URL normalisation, `ls-remote` parsing) are tested as data.
