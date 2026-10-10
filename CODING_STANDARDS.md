# Coding standards

Read during review. Formatting, clippy and the doc-link check are enforced by `scripts/check`; this file holds the judgement calls.

## Docs move with the code

A change that alters behaviour updates, in the same change, every doc that describes it: `GLOSSARY.md`, the ADRs, `docs/spec/spec.md`, and doc comments on the items it touches. Review greps the old names and phrases it removed. Merge or delete a superseded ADR rather than adding a new one beside it.

## Where decisions live

`plan` decides and returns data; `sync` runs typed actions through `Source` and reports; `cli` parses arguments and prints a typed result (ADR 0006). A `match` on policy in `sync` or `cli` belongs in `plan`. The follow-up `Hint` is not policy: it records where a run stopped, so `sync` sets it there. Completing a request (prompting for missing flags, refusing an empty URL, ADR 0008) is input handling in `cli` and `prompt`, not policy for `plan`. So is asking, or warning, about an input that maps to nothing (a version with no tag, spec §7.2).

One rule decides whether a Pin or Checkout matches a Repo. Do not write a second comparison.

## Output

Status lines go to stderr, lowercase, in one wording style; data goes to stdout; `-q` silences status lines but never problems or incomplete-project hints (ADR 0005).

## Tests

Assert what the user sees: stdout, stderr, exit code, files on disk, the Plan as data. Use the two seams: the binary or `cli::run_with` (`tests/cli*.rs`), and `sync`/`edit` against `FakeSource`. No tests on internal helpers. Tests never touch the network.

Scripted stand-ins, each given to `cli::run_on`:

- `refs update`: `update::fake::FakeUpdater` (`tests/cli_update.rs`). `cli::run_with` gets `update::Unmanaged`, so only `cli::run` reaches axoupdater.
- Registry lookups: `registry::fake::FakeRegistry` (`tests/cli_registry.rs`). The adapters are tested through their pure `found(body)` functions against recorded response bodies (`tests/registry.rs`).
- Prompts: a scripted `Prompter` (`tests/cli_prompt.rs`).
- Packages inferred on `add`: `packages::infer_packages` over temp directories (`tests/packages.rs`); the add pipeline through `cli::run_with` with a scripted `Prompter` (`tests/cli_add_packages.rs`).
- Package lockfiles: `registry::used::used_versions` is tested as data per format (`tests/registry_used.rs`), `registry::used::find` and `worktree::candidate_dirs` over temp directories (`tests/registry_used_find.rs`, `tests/worktree.rs`).
- Tooling gaps: `tooling::gaps` over temp directories (`tests/tooling.rs`); the note on `sync`, `add` and `--check` through `cli::run_with` (`tests/cli_output.rs`).
- Tags: `Source::tags` is a row of the `Source` contract (`tests/contract.rs`), run against the fake and git; `registry::tag::tag_for` is tested as data (`tests/registry_tag.rs`).

Three adapters have no test. After changing one, check it by hand:

- `update::Axo`: `refs update` against a real installer install.
- `registry::Http`: `refs add --no-sync` with `npm:`, `cargo:` and `pypi:` names against the real registries.
- `prompt::Terminal` (`inquire`): `refs add` in a real terminal; check Enter, Tab and Esc (Esc cancels: checked).
