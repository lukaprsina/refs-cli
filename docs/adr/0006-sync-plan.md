---
status: accepted
supersedes: 0003, 0004
---

# `sync` is planned in two pure stages, grouped by Repo; `Source` only reports and executes

This is the one statement of how `sync` decides and acts. It replaces ADR 0003 (decisions live in `plan`, mechanics in `Source`) and ADR 0004 (two-stage plan), which had grown into a chain of refinements, and it changes the shape of the Plan: a flat `Vec<Action>` whose order carried implicit dependencies that only the executor knew became a Plan grouped by Repo, so the failure policy is data.

## Decisions in `plan`, mechanics in `Source`

`Source` has six methods: `resolve`, `verify`, `inspect`, `list` (the names in the references directory), `materialise` and `remove`. `inspect` returns an `Observed` (`Absent`, `Dangling`, `Foreign` or `At { pin, paths, dirty_files }`) and `plan` decides what to do with it: recreate a dangling Checkout, refuse a foreign or dirty one unless forced, move or remove, re-resolve or reuse a pin. `GitSource` has no opinions about what should happen; it classifies what is on disk and executes single actions. Where mechanics force a step, `materialise` takes it without being told: a Checkout of another remote cannot move, because its objects are in another Cache, so it is replaced; a Foreign or Dangling directory is an error, since `plan` removes a Dangling one first and never materialises over a Foreign one. The fake `Source` is the one truth about the disk in tests.

`verify` stays separate from `resolve` because pin reuse skips `resolve` (a floating ref keeps its SHA when `paths` change) while edited `paths` and `start` must still be checked against that SHA.

## Two stages

Which Repos re-resolve is a pure decision, but the pins it produces exist only after `resolve` has run, so one `plan` over (config, Lock, `Observed`) cannot also decide the checkouts. Planning is two pure functions with the executor between them.

1. `plan_lock(active, lock, flags)` decides which Repos to resolve or reuse (pin reuse keyed on url, ref and source; `--upgrade`; newly active). Verification is not a decision: the executor verifies every active Repo. `lock_drift(active, lock)` is the stale check on its own, which `sync --check` needs. With `--offline`, a missing or stale Lock, or any `--upgrade`, is a refusal here.
2. `plan_checkouts(active, lock, checkouts, project, force)` returns a `Plan`.

`refs lock` is stage 1 plus a write. `sync` always runs stage 1 (with a current Lock every step is a reuse, but every Repo is verified, and the Lock is written only if it changed), then stage 2. `sync --check` runs stage 2 only, against the existing Lock; a missing or stale Lock is out of date without resolving.

## The Plan is grouped by Repo

```
Plan { repos: Vec<RepoAction>, writes: Vec<WriteAgentFile>, exclude: Option<…>, refusals, notes }
```

- A `RepoAction` is one unit per Repo: `Materialise { repo, pin }` (create or move; `Source` tells which), `Replace { repo, pin, note }` (remove, then materialise: a Dangling Checkout, or a dirty one under `--force`) and `Remove { id }` (a Checkout of a name that is no longer active; a Checkout can outlive its config entry, so this takes an id). Actions for active Repos carry the `RepoRef`, so the executor never looks a Repo up again.
- The one dependency that used to be implicit, a Repo's materialise after its remove, no longer exists: it is one action, and its `note` (`recreated`) is reported by the executor only if the action succeeded.
- **Agent file writes depend on the checkouts they list.** A write runs only if every `Materialise` and `Replace` succeeded, and not after a failed one; a failed `Remove` of an inactive name does not hold it back, since the block does not list that Repo. Each Agent file is written independently. The rule is a method on the action type, so it is tested as data.
- Any refusal clears the writes at plan time (a block listing a Repo with no Checkout is the harm). The exclude rule is independent of both.
- `--check` means "the Plan has any repo action, write or exclude action, or a refusal". Notes do not count.
- `force` is a flag to `plan_checkouts`; `Source` takes no `force`. A Pin's `branch` is display-only, so `plan` compares pins with `Pin::same_commit` and `paths` as sets. `plan_checkouts` renders the block, splices it and plans a write only on a difference, so an in-sync project has an empty Plan. Malformed markers are `Refusal::Block`, which belongs to a file, not a Repo.

## What stage 2 observes

`Checkouts` is the observation of every Repo the plan has to judge: each active Repo, and each non-active name found in `references_dir` (from `Source::list`). It is built by one constructor that checks every active id is present and returns an error otherwise, so an omitted entry is not read as `Absent`. There is no separate listing. A non-active name that is `At` is removed (refused if dirty and not forced); `Foreign` or `Absent` is ignored. The old Lock is not read, so a retry after a failed stage 2 still finds them. `ProjectObserved` is the rest: the `references_dir` setting, the text of each Agent file and the exclude rule (`Present`, `Missing` or `NoGit`; `NoGit` is a note, not drift).

## Verification and offline

- `verify` runs for every active Repo on `lock` and `sync`, against the Cache, fetching commits and trees only on a miss. `start` and `paths` edits do not stale the Lock, so change detection cannot skip them. `--check` never verifies.
- `plan` ignores `--offline` in stage 2. `materialise(opts.offline)` errors, naming a missing object; other Repos proceed like any failure. There is no all-or-nothing pre-check, so an offline failure may leave some Repos updated. `--offline --check` never needs the Cache.

## Failures and exit codes

- Stage 1 collects all errors and writes the Lock only if there are none.
- Stage 2 runs sequentially and collects failures. A failure of one Repo does not stop the others (every action is idempotent, so a retry is safe); only the writes that list a failed Repo are held back. The run exits 1 with all diagnostics.
- A Lock that does not cover the active Repos is drift only under `--check`; after stage 1 it cannot happen, so a real `sync` reports it as a failure.
- `sync --check` exits 3 for plain drift and 1 for refusals: "run `refs sync`" is the wrong advice when `sync` will also refuse.

## Consequences

- The fake `Source` is stateful and in-memory: seedable with any `Observed`, able to inject failures per Repo and method, and mutated by `materialise` and `remove`. The failure policy is tested as Plan data; the executor tests only check that it does what the Plan says.
- The idempotence test is "apply a plan, re-plan, expect it empty".
- `doctor` reads `Checkouts`, `ProjectObserved` and the stage 2 Plan, and sees "this Repo will be replaced" instead of a sequence to interpret.
- Considered and rejected: a flat `Vec<Action>` with explicit `after` edges. It would have described the dependencies instead of removing them, and the only dependency that existed was remove-then-materialise.
