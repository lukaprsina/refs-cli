# `sync` plans in two stages: lock, then checkouts

Which Repos re-resolve is a pure decision, but the pins it produces exist only after `resolve` has run, so one `plan` over (config, Lock, `Observed`) cannot also decide the checkouts. Planning is split into two pure functions with the executor between them.

1. `plan_lock(active, lock, flags)` decides which Repos to resolve or reuse (pin reuse keyed on url/ref/source, `--upgrade`, newly active). Verification is not a decision: the executor verifies every active Repo. `lock_drift(active, lock)` is the stale check on its own, which `sync --check` needs. The executor runs it and produces the new Lock. With `--offline`, a missing or stale Lock, or any `--upgrade`, yields a refusal here.
2. `plan_checkouts(active, lock, Observed per Repo, ProjectObserved)` returns a `Plan`: the checkout, Agent file and exclude actions.

`refs lock` is stage 1 plus a write. `sync` always runs stage 1 (with a current Lock every step is a reuse, so nothing resolves, but every Repo is verified and the Lock is written only if it changed), then stage 2. `sync --check` runs stage 2 only, against the existing Lock; a missing or stale Lock is reported as out of date without resolving.

This refines ADR 0003. Decisions still live in `plan`, mechanics in `Source`.

## The Plan

A `Plan` is an ordered `Vec<Action>`: removals, then materialisations, then Agent file writes, then the exclude rule.

- Actions: `Remove`, `Materialise`, `WriteAgentFile`, `EnsureExclude`, `Refuse(Refusal)` and `Note(Note)` (typed enums in `diagnostic`, each a miette diagnostic). `Materialise` has no variants: `Source` tells creating, moving and sparse updates apart from what is on disk. A dangling Checkout is `Remove`, `Materialise` and a `Note(recreated)`. A `Note` is an info-level announcement of an autofix; only `recreated` exists for now.
- `force` is a flag to `plan_checkouts`: with it, a dirty Checkout becomes `Remove` plus `Materialise` instead of `Refuse`. `Source` takes no `force`.
- `--check` means "the plan contains any action other than a `Note`".
- `refs list` computes its own status table from `Observed`; it does not read the plan.
- `ProjectObserved` is plain data read by `sync` before stage 2: the `references_dir` setting, the text of each Agent file, the exclude rule (`Present`, `Missing` or `NoGit`; `NoGit` is a `Note`, not drift) and the directory names in `references_dir`, which `sync` gets from `Source::list` (the fake then stays the one truth about the disk). Removal candidates are the non-active names, each inspected by `sync`: `At` is `Remove` (`Refuse` if dirty), `Foreign` or `Absent` is ignored. The old Lock is not read, so a retry after a failed stage 2 still finds them.
- Any `Refuse` suppresses `WriteAgentFile` (not `EnsureExclude`). A marker refusal is `Refusal::Block`; it belongs to a file, not a Repo. Refusals sit after the materialisations and before the Agent file writes.
- A Pin's `branch` is display-only, so `plan` compares an `Observed` pin with `Pin::same_commit`, and `paths` as sets. `plan_checkouts` renders the block, splices it and emits `WriteAgentFile` only on a difference, so an in-sync project has an empty plan. Malformed markers become `Refuse`. There is no second trait; `Source` stays the only one.

## Verification and offline

- `verify` runs for every active Repo on `lock` and `sync`, against the Cache, fetching commits and trees only on a miss. `start` and `paths` edits do not stale the Lock, so they cannot be skipped by change detection. `--check` never verifies.
- `plan` ignores `--offline` in stage 2. `materialise(opts.offline)` is what errors, naming a missing object (the full listing is deferred); other Repos proceed like any failure. There is no all-or-nothing pre-check, so no extra `Source` method: an offline failure may leave some Repos updated. Revisit if that bites. `--offline --check` never needs the Cache.

## Failures and exit codes

- Stage 1 collects all errors and writes the Lock only if there are none.
- Stage 2 runs sequentially and collects failures. Other Repos still proceed (every action is idempotent, so a retry is safe), and the block is not rewritten if any Repo failed. The run exits 1 with all diagnostics.
- A Lock that does not cover the active Repos is drift only under `--check`; after stage 1 it cannot happen, so a real `sync` reports it as a failure rather than as out of date.
- `sync --check` exits 3 for plain drift and 1 for refusals (`foreign_dir`, `dirty_checkout`, malformed markers): "run `refs sync`" is the wrong advice when `sync` will also refuse.

## Consequences

- The fake `Source` is stateful and in-memory: seedable with any `Observed`, able to inject failures per Repo and method, and mutated by `materialise` and `remove`. Tests may also script it.
- The idempotence test becomes "apply a plan, re-plan, expect it empty".
- `doctor` reads `Observed`, `ProjectObserved` and the stage 2 plan.
