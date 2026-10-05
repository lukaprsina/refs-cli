---
status: accepted
---

# `sync` is planned in two pure stages with a third pure step between them, grouped by Repo; `Source` only reports and executes

This is the one statement of how `sync` decides and acts. `plan` decides everything that is a decision: which names are observed, how Lock entries are built, whether the Lock is written, which actions run, what a failure holds back (`settle`), and how stage 1's failures and stage 2's result become the run's outcome (in sync, out of date, refused, failed) and the order of its diagnostics (`conclude`: any stage 1 error fails the run; stage 1's errors come first). `sync` is a loop: it runs the typed actions through `Source`, collects the failures and builds the `Report` once after stage 2 (a run that stops earlier, in stage 1 or on a rejected edit, reports what stopped it). The Plan is grouped by Repo rather than a flat list of actions whose order carried implicit dependencies that only the executor knew, so the failure policy is data.

## Decisions in `plan`, mechanics in `Source`

`Source` has six methods: `resolve`, `verify`, `inspect`, `list` (the names in the references directory), `materialise` and `remove`. `inspect` returns an `Observed` (`Absent`, `Dangling`, `Foreign` or `At { pin, paths, dirty_files }`) and `plan` decides what to do with it: recreate a dangling Checkout, refuse a foreign or dirty one unless forced, move or remove, re-resolve or reuse a pin. `GitSource` has no opinions about what should happen; it classifies what is on disk and executes single actions. Where mechanics force a step, `materialise` takes it without being told: a Checkout of another remote cannot move, because its objects are in another Cache, so it is replaced; a Foreign or Dangling directory is an error, since `plan` removes a Dangling one first and never materialises over a Foreign one. The fake `Source` is the one truth about the disk in tests.

`verify` stays separate from `resolve` because pin reuse skips `resolve` (a floating ref keeps its SHA when `paths` change) while edited `paths` and `start` must still be checked against that SHA.

## Two stages and a step between

Which Repos re-resolve is a pure decision, but the pins it produces exist only after `resolve` has run, so one `plan` over (config, Lock, `Observed`) cannot also decide the checkouts. Planning is two pure functions with the executor between them, and for the failures of stage 1 a third pure step.

1. `plan_lock(active, lock, flags)` decides which Repos to resolve or reuse (pin reuse keyed on url, ref and source; `--upgrade`; newly active). Verification is not a decision: the executor verifies every active Repo. `lock_drift(active, lock)` is the stale check on its own, which `sync --check` needs. With `--offline`, a missing or stale Lock, or any `--upgrade`, is a refusal here.
2. `settle(active, old Lock, passed entries, failures, keep)` is the pure step after the executor has resolved and verified, and before stage 2. The failure policy `keep` is an input: `AllOrNothing` (nothing is written unless every Repo passed) or `Passing` (the entries that passed are written); `plan` does not know which edit asked for which. It returns the Lock to write, whether to write it (the policy keeps it and it differs from the Lock on disk), the `Coverage`, the errors in the order the failures came, and whether the edit is accepted, that is may be written to `refs.toml`. It is a separate step because Coverage depends on results that only exist after stage 1's I/O.
3. `plan_checkouts(coverage, lock, checkouts, project, force)` returns a `Plan`. `Coverage` is the active Repos the Lock covers (**Covered**) and the ids it could not lock (**Withheld**); a plain `sync` or `--check` passes full coverage.

`refs lock` is stage 1 plus a write. `sync` always runs stage 1 with `AllOrNothing` (with a current Lock every step is a reuse, but every Repo is verified, and the Lock is written only if it changed), then stage 2. `sync --check` runs stage 2 only, against the existing Lock; a missing or stale Lock is out of date without resolving.

## The Plan is grouped by Repo

```
Plan { repos: Vec<RepoAction>, writes: Vec<WriteAgentFile>, exclude: Option<ExcludeAction>, refusals: Vec<Refusal> }
```

- A `RepoAction` is one unit per Repo: `Materialise { repo, pin, moving }` (create, or move when `moving`; decided in `plan` from what was observed), `Replace { repo, pin, moving, note }` (remove, then materialise: a Dangling Checkout, `moving` false, or a dirty one under `--force`, `moving` true) and `Remove { id }` (a Checkout of a name that is no longer active; a Checkout can outlive its config entry, so this takes an id). Actions for active Repos carry the `RepoRef`, so the executor never looks a Repo up again.
- The one dependency that used to be implicit, a Repo's materialise after its remove, no longer exists: it is one action, and its `note` (`recreated`) is reported by the executor only if the action succeeded.
- **Agent file writes depend on the checkouts they list.** A write runs only if every `Materialise` and `Replace` succeeded, and not after a failed one; a failed `Remove` of an inactive name does not hold it back, since the block does not list that Repo. Each Agent file is written independently. The rule is a method on the action type, so it is tested as data.
- Any refusal clears the writes at plan time (a block listing a Repo with no Checkout is the harm). The exclude rule is independent of both.
- `--check` means "the Plan has any repo action, write or exclude action, or a refusal". Notes do not count.
- `force` is a flag to `plan_checkouts`; `Source` takes no `force`. A Pin's `branch` is display-only, so `plan` compares pins with `Pin::same_commit` and `paths` as sets. `plan_checkouts` renders the block, splices it and plans a write only on a difference, so an in-sync project has an empty Plan. Malformed markers are `Refusal::Block`, which belongs to a file, not a Repo.

## What stage 2 observes

`Checkouts` is the observation of every Repo the plan has to judge: each active Repo, and each non-active name found in `references_dir` (from `Source::list`). It is built by one constructor, `Checkouts::observe(active, listing, inspect)`, which decides the names and calls `inspect` for each, so a missing observation cannot be represented and an omitted entry cannot be read as `Absent`. There is no separate listing. A non-active name that is `At` is removed (refused if dirty and not forced); `Foreign` or `Absent` is ignored. The old Lock is not read, so a retry after a failed stage 2 still finds them. `ProjectObserved` is the rest: the `references_dir` setting, the text of each Agent file and the exclude rule (`Present`, `Missing` or `NoGit`; `NoGit` is a note, not drift).

## The Lock records the commit, not the Paths

A Lock entry is `id` plus a Pin (`source`, `url`, `ref`, `sha`, `branch`). It has no `paths`: they do not change which commit a Repo resolves to, and `verify` checks `paths` and `start` against that commit on every sync anyway. A Lock is stale when a Repo is added or removed or its url or Ref changed, so a `paths`-only edit (including reordering or repeating entries) leaves `refs.lock` byte for byte as it was, and `sync --check`, `sync` and `list --status` agree. `paths` is a set everywhere.

Two rules decide matching, both defined in `source`: `Pin::drift_from(repo)` (url and Ref) for Lock drift, pin reuse and the checkout stage's input, and `Observed::matches(repo, locked)` (same commit as the locked Pin, `paths` as a set) for the checkout stage and `list --status`. Lock-entry construction takes the Pin straight from `resolve`.

The format version stays 1. A Lock holding `paths` is still read (unknown keys are ignored), is current if url and Ref match, and loses the keys the next time the Lock is written for another reason; nothing rewrites it just to drop them.

## Verification and offline

- `verify` runs for every active Repo on `lock` and `sync`, against the Cache, fetching commits and trees only on a miss. `start` and `paths` edits do not stale the Lock, so change detection cannot skip them. `--check` never verifies.
- `plan` ignores `--offline` in stage 2. `materialise(opts.offline)` errors, naming a missing object; other Repos proceed like any failure. There is no all-or-nothing pre-check, so an offline failure may leave some Repos updated. `--offline --check` never needs the Cache.

## Failures and exit codes

- Stage 1 collects all errors. Under `AllOrNothing` (`lock`, `sync`, `add`, `enable`) the Lock is written only if there are none; under `Passing` (`remove`, `disable`) the entries that passed are written.
- Stage 2 runs sequentially and collects failures. A failure of one Repo does not stop the others (every action is idempotent, so a retry is safe); only the writes that list a failed Repo are held back. The run exits 1 with all diagnostics.
- A Lock that does not cover the active Repos is drift under `--check` (no Plan is made, and `check_outcome(None, ..)` is out of date); after stage 1 it cannot happen.
- `sync --check` exits 3 for plain drift and 1 for refusals: "run `refs sync`" is the wrong advice when `sync` will also refuse.

## No active Repo

With no active Repo the Plan removes the Managed block, markers included, from every Agent file that has one. The rest of the file is untouched and the file is kept even if it is left empty; a file without markers, or a missing one, is left alone. `sync --check` reports a leftover block as out of date, and is in sync once it is gone.

## Edits: one operation, one failure policy

`sync::edit(root, edit, no_sync, make_source, flags)` is the one operation behind `add`, `remove`, `enable` and `disable`; the CLI parses arguments and prints the `Edited` it returns. `refs.toml` is read once, and the edited text is parsed once into the config the sync runs against. `Edit::apply` is text in, text out (with the Group it had to create), and validates its own result, and `--no-sync` is a flag on the operation: with it no `Source` is made and the edit is only written. The result is typed: `Change` (`Unchanged`, `Written`, `Rejected`), the Group created and the `Report`. No-op detection (`after == before`) and the written/rejected outcome exist only here.

- **`add` and `enable` are atomic.** They grow the active set, so stage 1 runs against the edited config and `refs.toml` is written only if every Repo passes. An unresolvable Repo writes nothing, and the error names it.
- The mapping from edit to policy is one line in `sync::edit` (`Edit::grows`): `add` and `enable` use `AllOrNothing`, `remove` and `disable` use `Passing`.
- **`remove` and `disable` always write.** They can only shrink the set, so a Repo they did not touch must not block them: stage 1 keeps going past a failure, the Lock gets the entries that passed, `refs.toml` is written, and each failing Repo is reported with exit 1. Stage 2 then runs against that partial Lock: `plan_checkouts` takes the `Coverage` that `settle` returned (the Repos the Lock covers and the ids it could not lock, `withheld`), so the Checkouts and the Managed block follow the Repos that did lock, the removed or disabled Repo's Checkout goes, and a withheld Repo's Checkout is left alone (neither made, moved nor removed; it is not in the block). If every active Repo is withheld the block is left as it is. The outcome is Failed either way; `refs sync` finishes the withheld Repo once it is fixed.
- Every Repo-specific error names the Repo id (`SourceError::for_repo`; the code and help stay the failure's own). "refs.toml was not changed" is printed only for a `Rejected` edit, and "updated" when it was written.
- **Groups follow their Repos.** `add --group x` for a missing Group appends a bare `[groups.x]` (no name or description; the heading falls back to the id) and reports it. `remove` of a Group's last Repo removes the Group again when it has no description, so add and remove round-trip.

## Consequences

- The fake `Source` is stateful and in-memory: seedable with any `Observed`, able to inject failures per Repo and method, and mutated by `materialise` and `remove`. The failure policy is tested as Plan data; the executor tests only check that it does what the Plan says.
- The idempotence test is "apply a plan, re-plan, expect it empty".
- `doctor` reads `Checkouts`, `ProjectObserved` and the stage 2 Plan, and sees "this Repo will be replaced" instead of a sequence to interpret.
- Considered and rejected: a flat `Vec<Action>` with explicit `after` edges. It would have described the dependencies instead of removing them, and the only dependency that existed was remove-then-materialise.
