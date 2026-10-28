# Coding standards

Read during review. Formatting, clippy and the doc-link check are enforced by `scripts/check`; this file holds the judgement calls.

## Docs move with the code

A change that alters behaviour updates, in the same change, every doc that describes it: `GLOSSARY.md`, the ADRs, `docs/spec/spec.md`, and doc comments on the items it touches. Review greps the old names and phrases it removed. Merge or delete a superseded ADR rather than adding a new one beside it.

## Where decisions live

`plan` decides and returns data; `sync` runs typed actions through `Source` and reports; `cli` parses arguments into plain values and strings the steps together per command (ADR 0006): load the project, build the `Source` if the command syncs, run, and print the typed result through `ui` (`print_report`, `Console`). A `match` on policy in `sync` or `cli` belongs in `plan`; the one exception is `sync::apply` holding back the Agent file writes when a `Materialise` fails. The follow-up `Hint` is not policy: it records where a run stopped, so `sync` picks it from that stop (`sync/report.rs`). A fact about an edit comes back in `edit::Applied` (the Group it created); one that compares the config before and after (whether it gave the Project its first active Repo) is worked out in `sync::Edited`.

Per command:

- `cli::dispatch` has one arm per clap command (`remove`, `enable` and `disable` share `cli::edit`): `load` (`init` and `self update` need no project; `init::run` and `self_update::run` own theirs), `source` (only if the command syncs), and the `run_*` bodies that map what came back to words and an exit code. None holds policy about what to do.
- `add`: completing a request (prompting for missing flags, refusing an empty URL, ADR 0008) is input handling in `add::run` (with its questions in `add::ask`), not policy for `plan`; `cli::run_add` only prints what it did. So is asking, or warning, about an input that maps to nothing (a version with no tag, spec §7.2): `add::pin` does it, called from `add::run`, with its questions through `Prompter` and its lines through `Say`.
- `update` holds the Ref policy of `refs update`, in two stages (`update/assess.rs`; `update.rs` is the driver): `find` learns what each Repo comes to from the remote and the lockfile alone (no question, no outline), so the bare-`update` picker can label every Repo, and `settle` runs only for a Repo named or picked (the dirty hold, the version question, `mend`, `ask_ref`). `--list` stops after `find`: `listing` prints each label and, through the same `look` and `hold_of` that `mend` and `settle` use, what the move would break and why it would be skipped, and asks nothing. `find` holds the tag, branch and commit cascade in `follow`, and `by_remote`, and the rule that a Repo whose Checkout is dirty or cannot be inspected is held back from a move, not from a note (`hold_of`, a `Hold`); what a new Ref breaks is asked in `mend` with `update::ask::fill_update` over the advisory `listing::gone` (`Source::verify` stays the check that must pass), and a Ref gone from the remote is asked again in `ask_ref`; it composes the pure `used::answer`, `tag_for` and `newest` (the newest tag of its own kind, when no lockfile answers) and does not call `add::pin::pin`, only shares its version question (`choose_version`).

One rule decides whether a Pin or Checkout matches a Repo. Do not write a second comparison.

## Output

Status lines go to stderr, lowercase, in one wording style; data goes to stdout; `-q` silences status lines and advice (tooling gaps and the editor hint, spec §7.6) but never problems or incomplete-project hints (ADR 0005).

## Tests

Assert what the user sees: stdout, stderr, exit code, files on disk, the Plan as data. Use the two seams: the binary or `cli::run_with` (`tests/cli*.rs`), and `sync`/`edit` against `FakeSource`. No tests on internal helpers. Tests never touch the network.

Scripted stand-ins, each given to `cli::run_on`:

- `refs self update`: `self_update::fake::FakeUpdater` (`tests/cli_self_update.rs`). `cli::run_with` gets `self_update::fake::Unmanaged`, so only `cli::run` reaches axoupdater.
- Registry lookups: `registry::fake::FakeRegistry` (`tests/cli_registry.rs`). The adapters are tested through their pure `found(body)` functions against recorded response bodies (`tests/registry.rs`); `registry::Http` through its pure `request_url` and `interpret` (`tests/registry_http.rs`).
- Prompts: the scripted `Prompter` shared by the tests (`tests/common/script.rs`, used by `tests/cli_prompt.rs`).
- Packages inferred on `add` (and the vanished ones on `update`): `packages::manifest_paths` and `packages::infer` as data over paths and text (`tests/packages.rs`), read through `manifests::Manifests`; the add pipeline through `cli::run_on` with a scripted `Prompter` and a `FakeSource` that serves the Manifests (`tests/cli_add_packages.rs`).
- The `paths` and `start` pickers: `listing` (the cone rule, the docs-like rule, caps, dropping children) as data over paths (`tests/listing.rs`); the pickers through `cli::run_on` with `Answer::Pick` in the scripted `Prompter` (`tests/cli_add_pick.rs`); the order the picker lists matches in, `ui::path_scorer`, by what a person types (`tests/path_scorer.rs`).
- The summary and `Customize?` gate of `add`: through `cli::run_on` with a scripted `Prompter` and a `FakeSource` (`tests/cli_add_summary.rs`).
- Package lockfiles: `registry::used::used_versions` is tested as data per format (`tests/registry_used.rs`), `registry::used::answer` and `worktree::candidate_dirs` over temp directories (`tests/registry_used_find.rs`, `tests/worktree.rs`).
- Tooling gaps: `tooling::gaps` over temp directories (`tests/tooling.rs`); the note on `sync`, `add` and `--check` through `cli::run_with` (`tests/cli_output.rs`), which also covers the editor hint of `init` and of the edit that gives a Project its first active Repo.
- What `add` pins a registry package to: `add::pin::pin` over temp directories, a scripted tag list and scripted answers, including its wording and questions (`tests/add_pin.rs`); and end to end through `cli::run_with` (`tests/cli_registry.rs`).
- `refs update`: through `cli::run_on` with a `FakeSource`, real Package lockfiles in a temp directory, and `registry::fake::Unused` (which knows no package: `update` never asks the Registry), each run starting from a synced project (`tests/cli_update.rs`). The Ref policy lives in `update` and is tested through the command, not as helpers.
- Tags: `Source::tags` is a row of the `Source` contract (`tests/contract.rs`), run against the fake and git; `registry::tag::tag_for`, `split` and `newest` are tested as data (`tests/registry_tag.rs`).

Three adapters have no test. After changing one, check it by hand:

- `self_update::Axo`: `refs self update` against a real installer install.
- `registry::Http`: `refs add --no-sync` with `npm:`, `cargo:` and `pypi:` names against the real registries.
- `ui::Terminal` (`inquire`): `refs add` in a real terminal; check Enter, Tab and Esc (Esc cancels: checked), and the `pick` multi-select (type to filter, Space to select), in a short terminal (about 10 rows) too: the page must not overflow (`page_size` in `ui/terminal.rs`).
