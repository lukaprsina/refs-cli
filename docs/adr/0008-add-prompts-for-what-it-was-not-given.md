---
status: accepted
---

# `add` prompts for what it was not given, on a terminal

ADR 0005 has `refs` follow uv, and uv never prompts. `refs add` departs from that on purpose: in a terminal, it asks for the values it was not given, with `inquire`. `--no-input` turns this off.

## Decision

- **Gate.** Prompt only when stdin and stderr are both terminals and `--no-input` is not set. `run` decides this once, next to `Terminal`, so nothing below it reads the environment. CI, pipes and coding agents are never terminals, so they never block on a prompt.
- **Fill, don't replace.** A flag skips only its own prompt. An empty answer means absent, the same as leaving the flag out. `--no-input` is the way to be asked nothing.
- **What is asked.** With a URL given: `id` (default: last URL segment), `group` (a select over the existing Groups, shown only if there are any), `ref` and `description`, then one confirm for `paths`, `packages` and `start`. A bare `refs add` asks for the URL first. Without a terminal it is a usage error (exit 2).
- **Validation.** The URL, id and new group name are checked with the same rules as `config::parse`, so a bad answer is re-asked on the spot. The free-text answers (ref, description, paths, packages, start) are left to `config::parse`, which stays the final authority and rejects them after the edit, as it does for flags.
- **Cancel.** Esc or Ctrl-C changes nothing, prints "cancelled" (even with `-q`) and exits 1; a terminal that fails prints why and exits 1. No new exit code.
- **Echo.** The equivalent command line is printed as a status line (hidden by `-q`), so the flags get learned. There is no confirm before the sync; `--no-sync` exists.
- **`--no-input` is global**, like `-q`, so `remove`, `enable` and `disable` can reuse it. No environment variable for now.

## Why

- A bare `refs add <url>` asks for what the person would otherwise have to look up flag by flag, and `package.json` inference (spec §3.2, rank 1) will later fill `packages` and `start` by default, so those prompts become confirmations of inferred values.
- Gating on a terminal keeps the commands scriptable and agent-safe without a mode flag, which is what the uv-style CLI promises.

## Consequences

- The pure core (`edit`, `plan`, `sync`) never prompts. `cli.rs` fills an `AddRepo` before `Edit::Add` runs, and the `url` of `AddRepo` becomes optional on the command line (empty means absent), checked in `cli.rs`.
- Tests script the answers through the `Prompter` trait given to `cli::run_on`, with no pty.
- `inquire` is a new dependency (default features, for the fuzzy group select).
- This is the one recorded divergence from uv's no-prompt behaviour, beside those listed in ADR 0005.
