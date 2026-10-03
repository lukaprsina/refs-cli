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
| `source` | `trait Source`, six methods (the sixth, `list`, names the directories in the references directory). `resolve`, `verify` and `materialise` take a `RepoRef` (the `id` and its `Repo`; the `Repo` carries no id); `inspect` and `remove` take only the id, because a Checkout can outlive its config entry: `resolve(repo) -> Pin` (ref to commit, no fetch), `verify(repo, pin)` (Paths and Start exist at the commit), `inspect(id) -> Observed`, `materialise(repo, pin, opts)` (fetch, prefetch blobs, create the Checkout or make it again at the new commit or paths; `opts` carries offline) and `remove(id)`. `Observed` is `Absent`, `Dangling`, `Foreign` or `At { pin, paths, dirty_files }`. `GitSource` is the only real implementation and owns the Cache (`source::git::cache`: one bare blobless clone per normalised URL, each behind an exclusive `fd-lock` held for the whole operation), `ls-remote`, sparse worktrees (`source::git::checkout`: telling a Checkout of ours from a foreign or dangling directory by its `.git` file, and the record of its Pin and Paths kept in the worktree's admin directory) and file locks. `inspect` repairs a moved project's worktree links silently. `verify` and `materialise` fetch on a Cache miss unless `opts.offline` (`VerifyOpts`, `MaterialiseOpts`), and refuse a tag object (`refs::git::unpeeled_tag`). It makes no decisions about *what to do* with an `Observed`. | no | pure helpers as data; a few real-git contract tests |
| `lock` | Lock path (`refs.lock` at the root), read and write (a union tagged by `source`, which is required in every entry). No decisions. | yes, apart from reading and writing the file | data; temp directories for `read` and `write` |
| `render` | (active set, Lock, references dir) → block text with markers, or `NotLocked` if the Lock misses an active Repo. The preamble and Entries sit in `text` fences so no formatter re-wraps them. | yes | golden files, formatter-stability |
| `agent_file` | Marker parsing and splicing; refuses malformed markers. The block takes the line ending of the region it replaces. `read` is `None` for a missing file; `write` follows a symlinked file. | splice is pure | data; temp directories for `read` and `write` |
| `atomic` | Whole-file write via a temp file and rename, shared by `agent_file` and `lock`. | no | temp directories |
| `project` | Project root discovery (`find_root`), `load` (root, config, then the §6.1 output-path checks in `check_outputs`: inside the root once symlinks resolve, no broken symlinks, kinds), and `observe`, which reads the Agent files and the exclude rule into `ProjectObserved`, with `ensure_exclude` to write the rule. | no | temp directories |
| `plan` | The deep module, in two pure stages (ADR 0004). `lock_drift(active, Lock?)` is the stale check (a direct comparison, naming Repo and field), also used alone by `sync --check`. `plan_lock(active, Lock?, flags)` adds one `Step` per active Repo: `Resolve`, or `Reuse` of the locked pin with the config's `paths` (reuse is keyed on url and ref; `--upgrade` resolves floating refs again; a re-enabled Repo has no entry, so it resolves). The executor verifies every Repo, so verify is not a step. Refusals: `--offline` with drift, an unknown `--upgrade` id. `plan_checkouts(active, Lock, Observed per Repo, ProjectObserved, force)` returns an ordered `Vec<Action>`: `Remove`, `Materialise`, `WriteAgentFile`, `EnsureExclude`, `Refuse(Diagnostic)` (a `Foreign` or dirty Checkout unless `force`, malformed markers; it suppresses the Agent file write) and `Note` (`recreated`). It renders the block and writes it only if it differs. | yes | data; the stateful fake `Source` simulates every `Observed` state |
| `sync` | Sequences only; every file it touches belongs to `lock`, `agent_file` or `project`. Gets `ProjectObserved` from `project::observe`, runs `plan_lock`, executes it (resolve, verify, write the Lock), then runs `plan_checkouts` and applies the Plan, or diffs it for `--check` (any non-`Note` action is out of date). Stage 1 collects all errors; stage 2 continues past a failed Repo and skips the block write. | no | fake `Source`; asserts on plans, not on git |
| `doctor` | **Deferred.** Will be a registry of checks that reads `Observed` and the `sync --check` plan instead of re-inspecting, plus a few standalone checks. | mostly | one test per check, asserting on codes |
| `diagnostic` | The shared error contract: `thiserror` enums deriving `miette::Diagnostic`, with stable codes. Only config validation carries spans. | yes | codes, not message strings |
| `cli` | `clap` parsing, output, exit codes. No logic. `run(args, cwd, make_source)` takes a function from the loaded project root and config to the `Source`, so tests return the fake and `main` builds a `GitSource` (cache root, `<root>/<references_dir>`). `GitSource` implements the whole trait (`source::git`, pure helpers in `source::git::remote`). | no | a few end-to-end runs |

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
- **No configurable Preamble.** refs owns it, so the same config, Lock and `refs` version give the same block.
- **No user-facing abstraction over Sources.** Users see Repos and git terms; "Source" is a code-level name until a second kind exists.
