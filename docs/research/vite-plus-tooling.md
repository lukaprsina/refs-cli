# Vite+ tooling detection and ignore handling

Source: `voidzero-dev/vite-plus` at commit `d9eebf076597095f871d784f3dc029058507f6a5`, local clone `C:\Luka\code\rust\vite-plus`. Paths below are relative to that repo. Background docs: `docs/private/vp-migrate.md` in the refs-cli main checkout.

Scope note: Vite+ is a migrator and toolchain owner. Its code never tries to keep a *directory* out of other tools; it converts the project's tools to its own (Oxlint, Oxfmt, `vite.config.ts`). The closest analogues to `refs` are under Q3 and Q6.

## 1. Package manager and workspace root

- Entry: `packages/cli/src/utils/workspace.ts:40-109` (`detectWorkspace`) calls the Rust napi binding `detectWorkspace` (`packages/cli/binding/src/package_manager.rs:124-166`).
- Workspace root: `vt_workspace::find_workspace_root(&cwd)` (`package_manager.rs:126`). `vt_workspace` is an external crate from `voidzero-dev/vite-task`, pinned at rev `7d69d65...` (`Cargo.toml:318`). Its source is **not in this repo**, so how it walks up is **not found**. Visible from the caller: it returns a root plus a `WorkspaceFile`; `is_monorepo` is true only for `PnpmWorkspaceYaml` or `NpmWorkspaceJson` (`package_manager.rs:141-144`); `PackageJsonNotFound` means "no root" and returns all-`None` (`:128-135`). The root is "where the package.json file is located" (`:112`).
- Package manager, in order (`crates/vp_pm_cli/src/package_manager.rs:344-445`):
  1. `packageManager` field in package.json (exact version)
  2. `devEngines.packageManager`
  3. `pnpm-workspace.yaml` -> pnpm
  4. `pnpm-lock.yaml` -> pnpm
  5. `yarn.lock` or `.yarnrc.yml` -> yarn
  6. `package-lock.json` -> npm
  7. `bun.lock` or `bun.lockb` -> bun
  8. `.pnpmfile.cjs` / `pnpmfile.cjs` -> pnpm; `bunfig.toml` -> bun; `yarn.config.cjs` -> yarn
  9. caller-supplied default, else `UnrecognizedPackageManager` error
- Lockfile use for detection is **existence only** (`is_exists_file`). The version returned for lockfile/config detection is the literal `"default"` (doc comment `:339-343`).
- Workspace members are not delegated to the package manager. Vite+ reads patterns itself: `pnpm-workspace.yaml` `packages`, else package.json `workspaces` (array or `{packages}`), then globs `<pattern>/package.json` ignoring `**/node_modules/**` (`workspace.ts:66-83`, `:120-161`).
- Lockfile *content* is read in three places, none for detection: `package-lock.json` is parsed as JSON and edited to drop stale vite entries (`packages/cli/src/migration/npm-reinstall.ts:90-135`; a malformed file is skipped silently, `:123-128`); `yarn.lock` is substring-searched for `__metadata:` to detect Yarn Berry (`packages/cli/src/utils/preview-registry.ts:48-58`); lockfiles are skipped when scanning sources (`packages/cli/src/migration/migrator/vitest-v5.ts:794-797`).

## 2. Detecting tools

Two mechanisms, both in the TS migrator.

**Config file presence** - `packages/cli/src/migration/detector.ts:62-216` (`detectConfigs`). First existing file per tool wins; `fs.existsSync` in the project directory only (no ancestor walk).

| Tool | Names checked | Lines |
|---|---|---|
| Vite | `VITE_CONFIG_FILES` (`utils/constants.ts`) | 65-70 |
| Vitest | `vitest.config.{ts,mts,cts,js,mjs,cjs}` | 74-87 |
| tsdown | `tsdown.config.{ts,mts,cts,js,mjs,cjs,json}`, `tsdown.config` | 91-107 |
| tsup | `tsup.config.{ts,mts,cts,js,mjs,cjs,json}`; package.json `tsup` key | 29-37, 113-118, 202 |
| Oxlint | `.oxlintrc.json`, `.oxlintrc.jsonc` | 122-128 |
| Oxfmt | `.oxfmtrc.json`, `.oxfmtrc.jsonc` | 132-138 |
| ESLint flat | `eslint.config.{js,mjs,cjs,ts,mts,cts}` | 142-155 |
| ESLint legacy | `.eslintrc`, `.eslintrc.{json,js,cjs,yaml,yml}` | 159-172 |
| Prettier | 18 names: `.prettierrc[.json/.jsonc/.yaml/.yml/.toml/.js/.cjs/.mjs/.ts/.cts/.mts]`, `prettier.config.{js,cjs,mjs,ts,cts,mts}`; package.json `prettier` key | 41-60, 175-180, 198-200 |
| `.prettierignore` | presence flag | 181-184 |
| Node version | `.nvmrc`; package.json `volta.node` | 187-209 |

**Dependency presence** - e.g. `detectEslintProject` (`migrator/eslint.ts:89-129`) and `detectPrettierProject` (`migrator/prettier.ts:19-57`): `eslint` / `prettier` in `dependencies` or `devDependencies` of the root package.json, and if absent, of any workspace package. A Prettier dependency with no root config produces a warning and is skipped (`prettier.ts:304-311`).

Not found: any detection of Biome, Ruff or Stylelint (they appear only in snapshot fixtures and `packages/cli/src/help.ts` text); `tsc` as a tool (only `tsconfig*.json` files in the project root are located, `utils/tsconfig.ts:33-42`, for `baseUrl` / `types` cleanup, never for include/exclude).

Hook tools are detected too: husky by dependency, `husky` key, `prepare` script or `.husky/` (`migrator/git-hooks.ts:64-74`); `simple-git-hooks`, `lefthook`, `yorkie` (`:26`).

## 3. Ignore patterns, per tool

Vite+ does **not** add ignore entries for tools it detects. What it does:

- **ESLint**: ignores are delegated wholesale to the external `@oxlint/migrate`, run via `vp dlx` with `--merge --type-aware --with-nursery --details` (`migrator/eslint.ts:231-246`). Vite+ has no `.eslintignore` or `ignores` handling (the string `.eslintignore` appears nowhere in its `.ts`, `.rs` or `.md` files). Afterwards it deletes ESLint config files and strips deps (`:282-292`). If the subprocess fails: stderr is shown as a warning, a "run manually later" hint is logged, and the ESLint migration returns false (`:179-218`).
- **Prettier**: config converted by `vp fmt --migrate=prettier` (`migrator/prettier.ts:63-92, 122-134`). `.prettierignore` is **left alone with a warning**: "found - Oxfmt supports .prettierignore, but using the `ignorePatterns` option is recommended" (`:164-171`). Not merged, not deleted.
- **Oxlint / Oxfmt**: `.oxlintrc.json` / `.oxfmtrc.json` are merged into the `lint` / `fmt` keys of `vite.config.ts` (`migrator/vite-config.ts:242-301, 402-440`). Ignore patterns live in `lint.ignorePatterns` / `fmt.ignorePatterns` (`docs/guide/lint.md:30-40`, `docs/config/fmt.md:14`); no code adds to them.
- That merge is an **AST rewrite**, not a text append: `crates/vp_migration/src/vite_config.rs:63-105` uses ast-grep (TypeScript parser for both .ts and .js, `:22-24`) to insert the key into the exported config object. JSON/JSONC text is injected as a JS object literal (`:71-73`). Guard: if the key already exists (AST check), the JSON file is deleted as redundant (`vite-config.ts:416-426`). If the rewrite reports `updated: false`, it warns "Failed to merge X into Y" plus a manual step linking the docs, and leaves both files (`:437-445`).
- package.json edits use `JSON.parse` / `JSON.stringify` with detected indent and newline (`utils/json.ts:25-47`); comments would be lost.
- Editor config (`.vscode/settings.json`, Zed): a **text-preserving JSONC patch** with `jsonc-parser` `modify` / `applyEdits`; existing values win, only missing keys are inserted; comments, trailing commas and key order are preserved; no write when nothing changes (`utils/editor.ts:513-546`). Conflicts: interactive prompt Merge/Skip with default Skip (`:409-440`); non-interactive: merge for JSON/JSONC, skip for non-JSON (`:441-442`). The generated VS Code settings contain no `files.exclude`, `search.exclude` or `files.watcherExclude` (`:28-39`).
- **Textual appends to line-based ignore files** (the closest analogue to `refs`):
  - `.vitest/` is appended to the project's `.gitignore` when not already ignored (`migrator/vitest-v5.ts:976-991`). The check is `isDirectoryGitignored` (Rust, `crates/vp_migration/src/file_walker.rs:20-48`): builds a `GitignoreBuilder` for each `.gitignore` from the root down to the target's parent, deeper files win, and each ancestor directory is checked because a negation cannot re-include an ignored parent. It works for nonexistent directories and without a git repo (tests `:130-162`). The append adds a missing trailing newline first. The search root is the nearest Git root (`findGitRoot`) or the workspace root (comment at `vitest-v5.ts:972-975`).
  - `vp create` appends `node_modules` and dotenv lines to `.gitignore` (`packages/cli/src/create/utils.ts:216-262`): reads existing content, per-line `trim() ===` / regex presence check, prefixes `\n` if the file lacks a trailing newline, `fs.appendFileSync`. It also appends the un-ignore block `!.vscode/`, `!.vscode/settings.json`, `!.vscode/extensions.json` (`:268-296`) with a `trimEnd().endsWith(block)` idempotency check (`:292`).
- When it cannot edit safely: warn and leave (`.prettierignore`; husky / lefthook / simple-git-hooks / yorkie per `vp-migrate.md` "Git hook tools"); skip silently (malformed `package-lock.json`, `npm-reinstall.ts:123-128`); manual follow-up (failed vite.config merge); non-JSON editor files are skipped, not overwritten (`editor.ts:441-442`).

## 4. Does each tool read `.git/info/exclude`?

No table exists. The only evidence:

- `crates/vp_migration/src/file_walker.rs:20-22`: "Check a possibly nonexistent directory against repository-owned `.gitignore` files ... Do not use machine-local Git excludes: migration must produce ignore rules that also work for other contributors." So Vite+ deliberately does **not** count `.git/info/exclude` when deciding whether its own appended `.gitignore` line is needed. (`refs` is the opposite case: machine-local by design.)
- The same crate's source walker does honour it: `file_walker.rs:56-61, 87-93` (`WalkBuilder` with `.git_ignore(true)`, `.git_global(true)`, `.git_exclude(true)`, `.hidden(true)`, `.require_git(false)`), i.e. the Rust `ignore` crate.
- `vp fmt` help: "Path to ignore file(s). Can be specified multiple times. If not specified, .gitignore and .prettierignore in the current directory are used." (`packages/cli/src/help.ts:779-782`). It names the files Oxfmt reads by default, does not mention `.git/info/exclude`, and says only the current directory.
- ESLint, Prettier, Oxlint, Biome, tsc, Vitest behaviour with `.git/info/exclude`: **not found** in this repo. The docs say Oxlint discovers its config from `vite.config.*` (`docs/guide/lint.md:27`) and say nothing on ignore sources. Those facts need the upstream tools' own docs.

## 5. Reporting "left alone" and warnings

- Two helpers (`migrator/shared.ts:220-232`): `warnMigration(message, report?)` and `infoMigration(message, report?)`. With a `report`, the message is deduplicated into `report.warnings` / `report.manualSteps` (`migration/report.ts:13-36, 65-77`) and printed later; without a report it is logged immediately via `prompts.log.warn` / `.info`.
- Rendered at the end by `showMigrationSummary`: a "Warnings:" list (yellow `!`) then a "Manual follow-up:" list (blue arrow), one `  - ` bullet each (`migration/bin.ts:639-651`). The summary is shown if anything migrated or any warning exists (`bin.ts:1589`).
- Silencing: **no quiet/silent CLI flag found** for `vp migrate`. Options are `interactive, agent, editor, hooks, full` (`migration/options.ts`; `bin.ts:260-270`). `--no-interactive` removes prompts but warnings still print. An internal `silent` boolean on helpers suppresses success lines (`prettier.ts:176-193`) while warnings are routed to the report. Per-feature opt-outs exist: `--no-agent`, `--no-editor`, `--no-hooks` (`vp-migrate.md` Options). Warnings do not change the exit code; only a failed final install sets `process.exitCode` (`bin.ts:489-503`).
- Specific skip messages: `prompts.log.info("Skipped writing <path>")` (`editor.ts:458`) and "No changes needed for <path>" when idempotent (`editor.ts:535-540`).

## 6. Other useful findings

- Idempotent append recipe: `create/utils.ts:244-262` (per-line trimmed equality, newline-fix prefix, `appendFileSync`); `vitest-v5.ts:986-990` does read-modify-write with the same newline fix.
- The gap check respects nesting, negations and ignored parents (`file_walker.rs:23-48`); its test table (`:130-145`) covers anchored `/packages/unit/.vitest/`, comment lines, escaped `\!`, `!` re-includes and ignored parents.
- Source-scanner skip lists are hard-coded names, not ignore-file driven: `VITEST_SCAN_SKIP_DIRS` (`source-scan.ts:156-172`), `SKIP_DIRS` (`vitest-v5.ts:43-`), `OXLINT_RETENTION_SKIP_DIRS` = `node_modules .git .hg .svn` (`source-scan.ts:173`).
- Vite+ keeps its own hook scratch dir ignored by writing a `*` `.gitignore` inside it (`packages/cli/src/config/hooks.ts:550`).
- Tool detection is first-match per tool, project root only; workspace packages are checked only for dependencies (Q2), never for per-package ignore files.
- `vp migrate` requires the workspace root as target (`vp-migrate.md`, Target Path).

## Implications for ticket #60 (facts only)

- Vite+ has no detector for Biome, Ruff, Stylelint or `tsc` include/exclude, and no code that adds entries to `.eslintignore`, `.prettierignore`, ESLint `ignores` or `.oxlintrc`. It warns on `.prettierignore` and delegates ESLint to `@oxlint/migrate`.
- Its only line-based ignore edits are appends to `.gitignore` (`.vitest/`, `node_modules`, dotenv lines, `.vscode` un-ignores), each guarded by an existence check and a missing-newline fix.
- Its `.gitignore` gap check deliberately ignores `.git/info/exclude` because those rules are machine-local. The `refs` exclude rule is machine-local by design, so tools that read only `.gitignore` or their own ignore file are not covered by it.
- Vite+ gives no table of which tools read `.git/info/exclude`. In-repo evidence: the `ignore`-crate walker reads it; `vp fmt` defaults to `.gitignore` and `.prettierignore` in the cwd.
- JS/JSON config edits in Vite+ are AST rewrites (ast-grep on `vite.config`) or `jsonc-parser` text patches (editor settings: comment-preserving, existing values win); plain package.json edits reserialize with detected indent and drop comments.
- Package manager detection uses file existence only (priority order in Q1). `vt_workspace::find_workspace_root` internals live in the external `vite-task` repo and were not read.
- Warnings are collected, deduplicated and printed in a closing summary; no quiet flag exists and warnings do not affect the exit code.
