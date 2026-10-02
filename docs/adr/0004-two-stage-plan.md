# `sync` plans in two stages: lock, then checkouts

Which Repos re-resolve is a pure decision, but the pins it produces exist only after `resolve` has run, so one `plan` over (config, Lock, `Observed`) cannot also decide the checkouts. Planning is split into two pure functions with the executor between them.

1. `plan_lock(config, lock, flags)` decides which Repos to resolve, reuse or verify (pin reuse keyed on url/ref/source, `--upgrade`, newly enabled). The executor runs it and produces the new Lock. With `--offline`, a missing or stale Lock yields a refusal here.
2. `plan_checkouts(active, lock, Observed per Repo, ProjectObserved)` returns a `Plan`: the checkout, Agent file and exclude actions.

`refs lock` is stage 1 plus a write. `sync` runs stage 1 if the Lock is stale, then stage 2. `sync --check` runs stage 2 only, against the existing Lock; a missing or stale Lock is reported as out of date without resolving.

This refines ADR 0003. Decisions still live in `plan`, mechanics in `Source`.

## The Plan

A `Plan` is an ordered `Vec<Action>`: removals, then materialisations, then Agent file writes, then the exclude rule.

- Actions: `Remove`, `Materialise`, `WriteAgentFile`, `EnsureExclude`, `Refuse(repo, diagnostic)` and `Note(diagnostic)`. `Materialise` has no variants: `Source` tells creating, moving and sparse updates apart from what is on disk. A dangling Checkout is `Remove`, `Materialise` and a `Note(recreated)`. A `Note` is an info-level announcement of an autofix; only `recreated` exists for now.
- `force` is a flag to `plan_checkouts`: with it, a dirty Checkout becomes `Remove` plus `Materialise` instead of `Refuse`. `Source` takes no `force`.
- `--check` means "the plan contains any action other than a `Note`".
- `refs list` computes its own status table from `Observed`; it does not read the plan.
- `ProjectObserved` is plain data read by `sync` before stage 2: the text of each Agent file and whether the exclude rule is present. `plan_checkouts` renders the block, splices it and emits `WriteAgentFile` only on a difference, so an in-sync project has an empty plan. Malformed markers become `Refuse`. There is no second trait; `Source` stays the only one.

## Verification and offline

- `verify` runs for every active Repo on `lock` and `sync`, against the Cache, fetching commits and trees only on a miss. `start` and `paths` edits do not stale the Lock, so they cannot be skipped by change detection. `--check` never verifies.
- `plan` ignores `--offline` in stage 2. `materialise(opts.offline)` is what errors, naming the missing OIDs; other Repos proceed like any failure. There is no all-or-nothing pre-check, so no extra `Source` method: an offline failure may leave some Repos updated. Revisit if that bites. `--offline --check` never needs the Cache.

## Failures and exit codes

- Stage 1 collects all errors and writes the Lock only if there are none.
- Stage 2 runs sequentially and collects failures. Other Repos still proceed (every action is idempotent, so a retry is safe), and the block is not rewritten if any Repo failed. The run exits 1 with all diagnostics.
- `sync --check` exits 3 for plain drift and 1 for refusals (`foreign_dir`, `dirty_checkout`, malformed markers): "run `refs sync`" is the wrong advice when `sync` will also refuse.

## Consequences

- The fake `Source` is stateful and in-memory: seedable with any `Observed`, able to inject failures per Repo and method, and mutated by `materialise` and `remove`. Tests may also script it.
- The idempotence test becomes "apply a plan, re-plan, expect it empty".
- `doctor` reads `Observed`, `ProjectObserved` and the stage 2 plan.
