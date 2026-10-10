---
status: proposed
---

# A workspace of a shared core, the reference-repo tool and the skills tool, with the core extracted as the skills tool needs it

Skill sources (ADR 0010) get their own manifest, lock and verbs. The code follows: the crate becomes a Cargo workspace in which a core holds what both tools use, and each tool holds what only it uses.

## Decision

- **Three crates.** `refs-core` (library), `refs-cli` (the reference-repo tool, today's crate and the `refs` binary) and `refs-skills` (library, the skills tool). `refs-cli` depends on both libraries. Whether `refs skill ...` stays a subcommand of the `refs` binary or `refs-skills` gets its own `[[bin]]` is not decided: it is a packaging choice that costs little to change either way, and one binary means one cargo-dist artifact and one `refs update`.
- **The core is what both tools call.** The Pin and `Source`/`GitSource` with the Cache, the step 1b prefetch and worktrees; reading and atomically writing a lock file; the miette setup and the diagnostic codes' conventions; text-cut TOML editing (ADR 0007); project root discovery; the Exclude rule; the status-line output model (ADR 0005). Each tool keeps its own config schema, lock schema, plan and verbs.
- **Extract on demand, not up front.** `refs-skills` is built against the existing modules, and code moves into `refs-core` only when `refs-skills` imports it. A move passes the deletion test: delete the shared module and the same code reappears in both tools. Code that merely looks generic stays where it is. The core gains no type for "things both might need".
- **`Source` stays the seam.** The skills tool uses `Source` as it stands plus the discovery method and `place`/`unplace` that ADR 0010 adds. If it needs more than that, the core has the wrong shape, and that is learned from the first skills ticket rather than a design document.
- **The core does not know the tools.** It has no config type, no plan type and no verb. A dependency from `refs-core` to either tool is a defect.

## Why

- Two tools with two manifests, locks and plans that share a git layer are two consumers of one seam, which is the point where extracting it stops being hypothetical. One consumer would only make a grab-bag.
- It turns the architecture sweep ADR 0010 asked for ("`cli.rs` and `edit.rs` should be smaller before this lands") from cleanup into a prerequisite with a target: whatever the skills tool needs from them is what moves.
- It keeps the spec's core rule true of `refs`: the block is the whole product, and the one part of the family that is a decision point for the agent lives in its own crate.

## Consequences

- The first skills ticket includes the extractions it needs. Each is its own commit, with `refs-cli` passing its tests unchanged (the tests are the contract).
- `scripts/check`, CI and cargo-dist cover the workspace. If the skills tool gets its own binary, cargo-dist builds two artifacts and `refs update` updates only the one it was installed with.
- Public names in `refs-core` are an interface to keep small: the tools are in this repository and can change together, so nothing in the core is `pub` for a hypothetical third consumer.
- `GLOSSARY.md` marks which terms belong to the core (Pin, Source, Cache, Checkout, Exclude rule) and which to a tool.

## Not decided

- One binary or two.
- A crate name for the skills tool if it is ever published on its own.
- A local layer for Repos. The committed Managed block must not depend on a machine-local file (spec §8.2). A layer would need a second block in a file that is not committed (such as `CLAUDE.local.md`, through `agents_files`), which only some agents read, splits the lookup sentence across two sections and makes `sync --check` treat a file as optional. Not planned: a Repo someone needs goes into the committed `refs.toml`, and the team decides whether to keep it.
