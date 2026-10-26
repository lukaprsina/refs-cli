---
status: accepted
---

# `refs lock` is removed; `sync` writes the Lock and `update` moves floating Refs

`lock` was `sync` stage 1 plus a write, and `lock --upgrade` the scriptable move for floating Refs (ADR 0012 kept it for parity with `uv lock --upgrade`). `uv` has no `update`; `refs` has one, so two commands now did the same job and differed only in what they also touched.

- **`sync` is the one command that resolves Refs and writes the Lock.** It already ran stage 1 (ADR 0006), so a project that only needs a Lock gets a Checkout too.
- **`refs update` moves a floating Ref.** A script runs `update --list` to see what would move, then `update <id…>`. A Repo that needs a version choice still needs a terminal, as before.
- **No unattended "move every floating Repo" command.** ADR 0012 rejected `update --all` and a moving `--no-input`; this does not reopen that.
- **Gone with it:** `Upgrade::All` (the upgrade is now a list of ids), the unknown-id and offline-upgrade refusals (`refs::lock::unknown_id`, `refs::lock::offline_upgrade`). `update` only names active Repos and has no `--offline` option, so clap refuses it.

This supersedes the line "`refs lock --upgrade` stays" in ADR 0012. ADR 0005's list of `uv` names that `refs` copies no longer includes `lock`.
