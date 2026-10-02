# Architecture

How `refs` is put together, and where new features go. Vocabulary is in `CONTEXT.md`; the decisions behind the shape are in `docs/adr/` (0001 for the structure, 0002 for diagnostics, 0003 for where the sync decisions live, 0004 for the two-stage plan). Behaviour is specified in `docs/spec/spec.md`.

## Shape: a pure core with effectful edges

```
refs.toml ─▶ config ─▶ active(config) ─┐
refs.lock ─▶ lock (read) ──────────────┴─▶ plan_lock ─▶ resolve/verify ─▶ new Lock
                                                                            │
Source.inspect ─▶ Observed ──────────┐                                      ▼
agent files, exclude ─▶ ProjectObserved ─┴─▶ plan_checkouts(active, Lock) ─▶ Plan ─▶ apply
                                                    │                       (or diff, for --check)
                                                    ▼
                             (active repos, Lock) ─▶ render ─▶ agent file splice
```

`sync --check` skips `plan_lock` and plans against the existing Lock.

Everything above `source` is a plain function over data. Files, git and the network are touched in a few thin places, all listed below as impure.

| Module | Job | Pure? | Tested with |
|---|---|---|---|
| `config` | Parse and validate the Project config (including `url`/`ref` hardening), keeping spans for validation errors. Comment-preserving edits (`toml_edit`). | yes, apart from reading files | data: config text in, diagnostics or types out |
| `active` | Config → the active set (spec §6.4). Everything after it sees only this. | yes | data |
| `source` | `trait Source`, five methods: `resolve(repo) -> Pin` (ref to commit, no fetch), `verify(repo, pin)` (Paths and Start exist at the commit), `inspect(repo) -> Observed`, `materialise(repo, pin, opts)` (fetch, prefetch blobs, create or move the Checkout; `opts` carries offline and force) and `remove(repo)`. `Observed` is `Absent`, `Dangling`, `Foreign` or `At { pin, paths, dirty_files }`. `GitSource` is the only real implementation and owns the Cache, `ls-remote`, sparse worktrees and file locks. It makes no decisions about *what to do* with an `Observed`. | no | pure helpers as data; a few real-git contract tests |
| `lock` | Config hash and Lock read/write (a union tagged by `source`, which is required in every entry). No decisions. | yes | data |
| `render` | (Preamble, active Repos, Lock) → block text. | yes | golden files, formatter-stability |
| `agent_file` | Marker parsing and splicing; refuses malformed markers. Atomic write is a thin wrapper. | splice is pure | data |
| `project` | Project root discovery, the references directory, the exclude rule. | no | temp directories |
| `plan` | The deep module, in two pure stages (ADR 0004). `plan_lock(config, Lock, flags)` decides whether the Lock is current and which Repos resolve, reuse a pin (keyed on url/ref/source; `--upgrade`; re-enabled) or verify. `plan_checkouts(active, Lock, Observed per Repo, ProjectObserved)` returns an ordered `Vec<Action>`: `Remove`, `Materialise { how }` (`Create`, `Move`, `Recreate`, `UpdateSparse`), `WriteAgentFile`, `EnsureExclude`, `Refuse` (a `Foreign` or dirty Checkout unless forced, malformed markers) and `Note` (autofix announcements). It renders the block and writes it only if it differs. | yes | data; the stateful fake `Source` simulates every `Observed` state |
| `sync` | Reads `ProjectObserved`, runs `plan_lock`, executes it (resolve, verify, write the Lock), then runs `plan_checkouts` and applies the Plan, or diffs it for `--check` (any non-`Note` action is out of date). Stage 1 collects all errors; stage 2 continues past a failed Repo and skips the block write. | no | fake `Source`; asserts on plans, not on git |
| `doctor` | **Deferred.** Will be a registry of checks that reads `Observed` and the `sync --check` plan instead of re-inspecting, plus a few standalone checks. | mostly | one test per check, asserting on codes |
| `diagnostic` | The shared error contract: `thiserror` enums deriving `miette::Diagnostic`, with stable codes. Only config validation carries spans. | yes | codes, not message strings |
| `cli` | `clap` parsing, output, exit codes. No logic. | no | a few end-to-end runs |

One package, a library plus a thin binary. Split into a workspace only when a real reason appears (for example, `miette`'s `fancy` feature dominating build time).

## Seams

- **`Source` is the only trait.** It is injected wherever it is needed, so tests pass a fake, not a mocked module. Everything else is a function, because a trait with one implementation is speculative. If a second renderer or resolver ever appears, extracting a trait then is cheap.
- **The Lock is a union tagged by `source`.** Only `source` code interprets the pin; other modules treat it as opaque.
- **`active` is the selection seam.** The enabled flag is its first rule. Per-file placement of Groups would be another, and `render` already takes an Agent file plus the Entries selected for it.
- **Plan then apply, in two stages.** `--check` reads the same stage 2 plan a real `sync` applies, so they can't drift. Decisions live in `plan`, mechanics in `Source` (ADR 0003); the pins stage 2 needs come from executing stage 1 (ADR 0004).

## Where future features land

| Feature | Lands in | Changes elsewhere |
|---|---|---|
| Tarball or local-directory Source | new `Source` impl and a `source` kind in config and Lock | none downstream; the Lock change is additive |
| Ref from a package lockfile | a new resolver feeding `lock` | none downstream |
| Package inference on `add` | a helper in `add` that reads a Checkout | none |
| Inline tree, per-file hints | a renderer step and whatever per-Repo data it needs | `render` only |
| Per-file placement of Groups | a new selection rule beside `active` | none beyond selection |
| Nested Projects (monorepos) | `project` discovery; each Project runs the same pipeline | none |
| Linter and formatter exclusion | `project`, plus a `doctor` check | none |
| `--json` output | a second consumer of `plan` and the render input | `cli` |
| `gc` for the Cache | a `GitSource` method and a command | none |
| `doctor`, and more checks | one registry entry per check | none |
| Different Preamble wording | the Preamble, versioned with the generator | golden tests |

## What this deliberately doesn't do

- **No per-target block variants.** Every Entry renders identically wherever it appears; only the selection can differ.
- **No configurable Preamble.** refs owns it, so the same config, Lock and generator version give the same block.
- **No user-facing abstraction over Sources.** Users see Repos and git terms; "Source" is a code-level name until a second kind exists.
