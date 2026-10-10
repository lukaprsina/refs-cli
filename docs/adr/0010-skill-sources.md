---
status: proposed
---

# Skill sources are pinned git Repos whose resolved skills are placed, not rendered

A project can pin the skills it uses the way it pins reference repos: intent in `refs.toml`, the commit and the resolved skill list in `refs.lock`, and a placement of each skill where the agent looks for it. Research: `docs/private/08_AGENT-SKILLS-RESEARCH.md`. Extends ADR 0001 (the `source` tag was kept for a second kind of Pin) and ADR 0009 (read the Checkout, do not add a resolver).

## Decision

- **A new table, the same machinery.** `[skills.<id>]` has `url`, `ref`, `group`, `enabled` and a selection: `select = ["tdd", "grill-me"]` (skill directory names), `select = "*"`, or `plugins = ["name"]` (the skills a plugin manifest lists). `exclude = [...]` removes names from the result. The Pin is the same Pin (a commit of a remote), resolved and verified by the same `Source`; the Cache and `--offline` work unchanged. Ids are unique across `repos` and `skills`, so `remove`, `enable`, `disable` and `lock --upgrade-package` take either.
- **The Lock records the resolved list.** A `[[skill]]` entry holds the Pin and `skills = [...]`, the names the selection resolved to at that commit. `lock --upgrade-package <id>` re-resolves and reports `added skill <name>` and `removed skill <name>`; with an explicit `select` it also notes how many upstream skills are not selected. A skill is never added or dropped silently, and the author's repo is the unit of update.
- **A skill is a directory.** Its identity is the directory name, which is also the placement name (Claude Code does the same). A frontmatter `name` that differs warns and is ignored; the spec's name rules are not enforced, as no client enforces them. A missing `SKILL.md` or an unreadable frontmatter excludes the directory with a warning when discovered by tree listing, and is an error when the skill was named in `select`.
- **Discovery is a tree listing, then a prefetch.** `Source` gains one method that returns the `SKILL.md` files and the `.claude-plugin/*.json` manifests at a Pin: `GitSource` lists the tree (no blobs), prefetches exactly those blobs with the step 1b command, and reads them with `GIT_NO_LAZY_FETCH=1`. Parsing the frontmatter and the manifests is a pure function over the returned bytes, tested as data. Measured at about 2.4 s cold for 38 skills. The Checkout then uses the selected skill directories as its Paths.
- **Placement, not a block Entry.** Each selected skill is linked from its Checkout to `<skills_dir>/<name>` for every entry of `settings.skills_dirs` (project config, default `[".claude/skills"]`, validated like `agents_files`). Claude Code follows directory symlinks and does not read `.agents/skills`; no per-agent table. On Windows a directory junction, with a copy as the fallback (untested, open). The Managed block does not mention skills: the harness already lists their descriptions. Each placed name gets a line in the Exclude rule.
- **Placement lives behind `Source`** (ADR 0001: disk sits behind it), as `place` and `unplace`, and the Plan gains placement actions. A path in a skills dir that refs did not create is a Foreign directory and is never touched, so hand-written skills coexist. Two sources that select the same name, or a name already held by another source, are a refusal naming both.
- **CLI.** One new verb: `refs skill add <src> [--plugin <p>…] [--only <name>…] [--all]`, with `gh:owner/repo` input sugar and the id defaulting to `owner-repo`. `--all` is explicit, never a default, as every installed skill costs context. Everything else is shared.

## Why

- The three failures of the existing tool are all absent resolution: the author is not a unit, groups are interactive only, and `update` neither adds nor reports removed skills. Intent plus a recorded resolution fixes each without a concept of an author.
- Reading the tree reuses the Cache and step 1b; no new fetching logic. Placement by symlink keeps one copy per Checkout and follows the Checkout when it moves.
- A noun for `add` keeps `--paths`, `--packages` and `--start` from becoming mode-dependent flags.

## Consequences

- Skills are a decision point for the agent, the thing the spec's core rule argues against (spec §2.1). This ADR is for procedures, not reference knowledge; the spec should say so.
- The glossary needs Skill, Plugin (the marketplace's own term; Group is taken), Skill source and Placement. Spec §3 and §6 gain a section; the Lock format gains `[[skill]]` (additive, no `version` bump).
- A YAML parser is added for frontmatter (`serde-saphyr` or `serde_norway`; `serde_yaml` is deprecated) and `serde_json` for the manifests.
- `cli.rs` and `edit.rs` should be smaller before this lands.

## Not decided

- Selection by bucket directory (mattpocock's `engineering/`); v1 selects by name or plugin.
- Windows junctions, and plugin-versus-project precedence.
- User-level (global) skills; refs is project-scoped.
- Non-git sources (registry packages, `node_modules`).

## Cost and a smaller first cut

Rough estimate, by analogy to the registry work (plus or minus 2x): about 1,500 lines of code and 1,500 of tests in four or five tickets (config and validation; the `[[skill]]` lock entry and the added/removed report; discovery with the YAML crate; placement; `refs skill add`), plus spec, glossary and ADR updates. Roughly 250-400k output tokens, 10-20M tokens consumed in total with cached context. Windows junctions would add cost that cannot be tested on Linux.

The decision above is the full design. A smaller first cut keeps all three fixes (the author's repo as the unit of update, non-interactive selection, the added/removed report) at about two thirds of the cost:

- No `plugins` selector: mattpocock-skills has one plugin per repo, so it adds little over `select = "*"`.
- No copy fallback and no Windows junction work; symlink placement only.
- `select`, the recorded skill list, the diff report and `refs skill add --only/--all`.

Do it after the architecture sweep, and only if it will be used regularly. Open risks: the feature pulls refs toward a second product and is itself a decision point for the agent (spec §2.1); Vercel's tool or Claude Code plugins may add resolved lists; demand beyond the author is unchecked. A plain `[repos.*]` entry with `paths` already pins a skills repo and covers the update half without placement or a diff report.
