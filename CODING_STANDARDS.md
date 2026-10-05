# Coding standards

Read during review. Formatting, clippy and the doc-link check are enforced by `scripts/check`; this file holds the judgement calls.

## Docs move with the code

A change that alters behaviour updates, in the same change, every doc that describes it: `GLOSSARY.md`, the ADRs, `docs/spec/spec.md`, and doc comments on the items it touches. Review greps the old names and phrases it removed. Merge or delete a superseded ADR rather than adding a new one beside it.

## Where decisions live

`plan` decides and returns data; `sync` runs typed actions through `Source` and reports; `cli` parses arguments and prints a typed result (ADR 0006). A `match` on policy in `sync` or `cli` belongs in `plan`.

One rule decides whether a Pin or Checkout matches a Repo. Do not write a second comparison.

## Output

Status lines go to stderr, lowercase, in one wording style; data goes to stdout; `-q` silences status lines but never problems or incomplete-project hints (ADR 0005).

## Tests

Assert what the user sees: stdout, stderr, exit code, files on disk, the Plan as data. Use the two seams: the binary or `cli::run_with` (`tests/cli*.rs`), and `sync`/`edit` against `FakeSource`. No tests on internal helpers.
