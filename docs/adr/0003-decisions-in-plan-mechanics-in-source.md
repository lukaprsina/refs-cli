# Decisions live in `plan`; `Source` only reports and executes

`Source` has six methods: `resolve`, `verify`, `inspect`, `list` (the names in the references directory), `materialise` and `remove`. `inspect` returns an `Observed` value (`Absent`, `Dangling`, `Foreign` or `At { pin, paths, dirty_files }`) and `plan` decides what to do with it: recreate a dangling Checkout, refuse a foreign or dirty one unless forced, move or remove, re-resolve or reuse a pin. `GitSource` has no opinions about what should happen; it classifies what is on disk and executes single actions.

`verify` stays separate from `resolve` because pin reuse skips `resolve` (a floating ref keeps its SHA when `paths` change) while the edited `paths` and `start` must still be checked against that SHA.

This refines ADR 0001. The pure core now includes the decision table that spec §7.3 steps 2-4 describe, so it is tested with the fake `Source` simulating each `Observed` state instead of through slow real-git tests. The `lock` module shrinks to the config hash and Lock serialisation, and there is no `entry` module: `render` takes the active Repos and the Lock directly.

## Consequences

- The fake `Source` must be able to produce every `Observed` state, including `Dangling` and dirty files.
- `GitSource` contract tests cover classification (is this directory a worktree of this cache, is its gitdir missing) and the mechanics, not the policy.
- A future `doctor` reads `Observed` and the `sync --check` plan, so it needs no inspection code of its own.
