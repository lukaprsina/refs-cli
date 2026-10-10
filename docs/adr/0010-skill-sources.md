---
status: proposed
---

# Skill sources are pinned git repos with their own manifest and lock, and their skills are placed, not rendered

A project can pin the skills it uses the way it pins reference repos: intent in `skills.toml`, the commit and the resolved skill list in `skills.lock`, and a placement of each skill where the agent looks for it. It is a second tool beside the reference repos, with its own files; ADR 0011 says how the code is split. Research: `docs/private/08_AGENT-SKILLS-RESEARCH.md`. Extends ADR 0001 (the `source` tag was kept for a second kind of Pin) and ADR 0009 (read the Checkout, do not add a resolver).

## Decision

- **Its own manifest and lock.** `skills.toml` and `skills.lock` sit at the project root next to `refs.toml` and `refs.lock`, are committed, and are found the same way (walk up to the first directory holding one). `refs.toml` is unchanged and rejects a `[skills]` table as any unknown key. Ids are unique within the skills files only; a source and a Repo may share an id.
- **A source is a table.** `[sources.<id>]` has `url`, `ref`, `enabled` and a selection: `select = ["tdd", "grill-me"]` (skill directory names), `select = "*"`, or `plugins = ["name"]` (the skills a plugin manifest lists). `exclude = [...]` removes names from the result. There are no groups: a group is a block heading and nothing here renders a block. `[settings]` has `skills_dirs` (default `[".claude/skills"]`), validated like `agents_files`. The Pin is the same Pin (a commit of a remote), resolved and verified by the same `Source`; the Cache and `--offline` work unchanged. `remove`, `enable`, `disable` and `lock --upgrade-package` take source ids.
- **The lock records the resolved list.** A `[[source]]` entry in `skills.lock` holds the Pin and `skills = [...]`, the names the selection resolved to at that commit. `lock --upgrade-package <id>` re-resolves and reports `added skill <name>` and `removed skill <name>`; with an explicit `select` it also notes how many upstream skills are not selected. A skill is never added or dropped silently, and the author's repo is the unit of update. `skills.lock` has its own `version`.
- **A skill is a directory.** Its identity is the directory name, which is also the placement name (Claude Code does the same). A frontmatter `name` that differs warns and is ignored; the spec's name rules are not enforced, as no client enforces them. A missing `SKILL.md` or an unreadable frontmatter excludes the directory with a warning when discovered by tree listing, and is an error when the skill was named in `select`.
- **Discovery is a tree listing, then a prefetch.** `Source` gains one method that returns the `SKILL.md` files and the `.claude-plugin/*.json` manifests at a Pin: `GitSource` lists the tree (no blobs), prefetches exactly those blobs with the step 1b command, and reads them with `GIT_NO_LAZY_FETCH=1`. Parsing the frontmatter and the manifests is a pure function over the returned bytes, tested as data. Measured at about 2.4 s cold for 38 skills. The Checkout then uses the selected skill directories as its Paths.
- **Placement, not a block Entry.** Each selected skill is linked from its Checkout to `<skills_dir>/<name>` for every entry of `skills_dirs`. Claude Code follows directory symlinks and does not read `.agents/skills`; no per-agent table. On Windows a directory junction, with a copy as the fallback (untested, open). Skills write nothing to `AGENTS.md`: the harness already lists their descriptions, and the Managed block stays a function of `refs.toml` and `refs.lock`. Each placed name gets a line in the Exclude rule. Checkouts of skill sources live in `<references_dir>/.skills/<id>/`, outside the names a Repo can take (`[a-z0-9]`), so `refs sync` never mistakes them for orphans.
- **Placement lives behind `Source`** (ADR 0001: disk sits behind it), as `place` and `unplace`, and the Plan gains placement actions. A path in a skills dir that refs did not create is a Foreign directory and is never touched, so hand-written skills, and skills a person installed with another tool, coexist. Two sources that select the same name, or a name already held by another source, are a refusal naming both.
- **CLI.** `refs skill <verb>` mirrors the refs verbs with the same semantics (ADR 0005): `add`, `remove`, `enable`, `disable`, `list`, `lock`, `sync`. `refs skill add <src> [--plugin <p>…] [--only <name>…] [--all]` takes `gh:owner/repo` input sugar and defaults the id to `owner-repo`. `--all` is explicit, never a default, as every installed skill costs context. Whether this stays a subcommand of the `refs` binary or becomes its own binary is not decided (ADR 0011).

## Global skills and a local layer

- **No global skills.** Claude Code already has a personal scope (`~/.claude/skills`), and `skills -g` fills it. The tool would need a second lock location, a checkout store outside any project and placements into `~`, and the intent would no longer live in the project's repo. Spec §3.3 rejected the global template library for the same reasons. A skill a person places by hand is a Foreign directory and coexists (above), so personal skills are possible without refs.
- **A local layer, later.** `skills.local.toml` and `skills.local.lock` next to the project files, excluded per clone through the Exclude rule (not `.gitignore`, which is committed). Not in the first cut: build it once someone besides the author asks, after the skills core lands. Because skills render no block, the layer needs none of the restrictions a layer for Repos would (ADR 0011).
  - **Add-only.** An id that exists in both layers is an error. No override of a team pin, which would leave two pins for one id and make `lock --upgrade-package` ambiguous. `enabled = false` on a team source is the one override that may be added later.
  - **Two locks.** The committed `skills.lock` cannot hold entries for local sources. Staleness is computed per layer and each lock holds only its own layer's entries.
  - **Cost.** The edit verbs pick the file (`--local`), edit-then-sync atomicity spans two locks, and `list` shows an entry's origin. Roughly 25-35% on top of the first cut.
  - **Shape.** Config loading takes an ordered list of layers and returns one merged active set, each entry tagged with its origin. A user-level file (`~/.config/refs/skills.toml`) would be a third layer, not a new feature, and is built only on demand.

## Why

- The three failures of the existing tool are all absent resolution: the author is not a unit, groups are interactive only, and `update` neither adds nor reports removed skills. Intent plus a recorded resolution fixes each without a concept of an author.
- Reading the tree reuses the Cache and step 1b; no new fetching logic. Placement by symlink keeps one copy per Checkout and follows the Checkout when it moves.
- Skills are a decision point for the agent, the thing the spec's core rule argues against (spec §2.1). Keeping them out of `refs.toml`, `refs.lock` and the block keeps that rule true of refs: the block is still the whole product there. This ADR is for procedures, not reference knowledge, and the spec should say so.
- A separate manifest removes what one file forced: ids unique across two tables, `--paths`, `--packages` and `--start` turning mode-dependent, and a local layer that could not be allowed to change the committed block.

## Consequences

- The glossary needs Skill, Plugin (the marketplace's own term; Group is taken), Skill source and Placement. Spec §3 gains a pointer and a sentence saying refs does not render skills; the skills manifest and lock format are described in their own document.
- A YAML parser is added for frontmatter (`serde-saphyr` or `serde_norway`; `serde_yaml` is deprecated) and `serde_json` for the manifests, in the skills crate only.
- Project root discovery and the Exclude rule are used by both tools and move into the shared core (ADR 0011). `cli.rs` and `edit.rs` should be smaller before this lands.
- Users learn two files. The `refs.toml` template and `refs init` do not mention skills.

## Not decided

- Selection by bucket directory (mattpocock's `engineering/`); v1 selects by name or plugin.
- Windows junctions, and plugin-versus-project precedence.
- Non-git sources (registry packages, `node_modules`).
- One binary or two (ADR 0011).

## Cost and a smaller first cut

Rough estimate, by analogy to the registry work (plus or minus 2x): about 1,500 lines of code and 1,500 of tests in four or five tickets (manifest and validation; the `[[source]]` lock entry and the added/removed report; discovery with the YAML crate; placement; `refs skill add`), plus spec, glossary and ADR updates. Roughly 250-400k output tokens, 10-20M tokens consumed in total with cached context. The extraction in ADR 0011 is counted there, not here. Windows junctions would add cost that cannot be tested on Linux.

The decision above is the full design. A smaller first cut keeps all three fixes (the author's repo as the unit of update, non-interactive selection, the added/removed report) at about two thirds of the cost:

- No `plugins` selector: mattpocock-skills has one plugin per repo, so it adds little over `select = "*"`.
- No copy fallback and no Windows junction work; symlink placement only.
- `select`, the recorded skill list, the diff report and `refs skill add --only/--all`.
- No local layer.

Do it after the architecture sweep, and only if it will be used regularly. Open risks: the tool is itself a decision point for the agent (spec §2.1); Vercel's tool or Claude Code plugins may add resolved lists; demand beyond the author is unchecked. A plain `[repos.*]` entry with `paths` already pins a skills repo and covers the update half without placement or a diff report.
