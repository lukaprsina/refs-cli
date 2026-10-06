---
status: accepted
---

# `remove`, `enable` and `disable` edit `refs.toml` by text, not through a `toml_edit` document

Spec §6.3 says to use `toml_edit` for writes. `add` does (it renders the new table with it), but `remove`, `enable` and `disable` locate spans with `toml_edit` and splice the original text. Re-serialising a `toml_edit` document in their place was tried and rejected.

## Why

- **Comments belong to the table below them.** `toml_edit` stores every comment and blank line above a header as that table's prefix. `add` appends after the file's last comment, so the comment becomes the new table's prefix, and `doc.remove(id)` deletes it. `add` then `remove` no longer returns the file byte for byte (spec §10, §12), which is the guarantee that matters. Only text-cutting can tell a comment directly above the table (cut with it) from one that was there before (kept).
- **`toml_edit` rewrites bytes it was not asked to.** It ends the file with a newline and writes `\n` for the lines it emits, so a CRLF file or one without a final newline needs fix-ups after every edit.
- **The saving was small.** `disable` and `enable` through `toml_edit` removed about eight lines, and `remove` still needed the text cut.

## Consequences

- `remove` cuts the table, the comment lines directly above it and the one blank line before those; the rest of the file keeps every byte.
- A future review that proposes `toml_edit` for these edits should reopen this only if `toml_edit` can separate a table's own comments from the ones before it.
