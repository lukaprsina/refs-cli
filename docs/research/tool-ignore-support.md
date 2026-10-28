# Which tools already skip `.git/info/exclude` paths

Tools used: Exa `web_search_exa` and `web_fetch_exa` for all web research (no WebSearch/WebFetch). Local runs: `rg`, `fd`, `git`, `node`/`npm` (Prettier 3.9.10, ESLint 10.13.0 installed into a throwaway temp dir) and `uvx ruff` 0.17.3. All temp dirs were deleted afterwards. A second pass ran TypeScript 7.0.2 (native), Oxlint 1.90.0, Oxfmt 0.75.0 and Biome 2.6.3, installed with npm into a directory under `~/.cache` (the first pass's installs failed with `EDQUOT` on `/tmp`, which is RAM). Zed 1.25.3 (Windows, released 2026-10-23) was tried by hand by the maintainer. A third pass (2026-10-28, again under `~/.cache`, deleted afterwards) ran Black, ty, Stylelint, markdownlint-cli2 0.24.0 (markdownlint 0.42.0) and dprint 0.64.3, each at the latest release then, in a scratch git repo with only `/.references/` in `.git/info/exclude`, a bad file in `.references/x/` and a clean one in `src/`, plus controls with the exclude rule removed or a `.gitignore` added.

Question: `refs` writes `/.references/` to `<git-dir>/info/exclude` (spec §7.6). For which tools is that enough, so no "Tooling gap" note is needed? Today is 2026-10-28; versions are the ones current in docs fetched that day unless stated.

Test layout (for every local run): `git init` repo, `.git/info/exclude` = `/.references/`, a badly formatted `.js/.py/.css/.md/.ts` file inside `.references/dep/` and the same inside `src/`. A tool "honors" a source if the `.references` file is not reported.

## Summary

| Tool | `.gitignore` | `.git/info/exclude` | `.ignore` | Needs gap note? |
|---|---|---|---|---|
| Prettier 3.9.10 | yes, `./.gitignore` of cwd only | **no** (tested) | **no** (tested) | **yes** |
| ESLint 10 flat | **no** (tested) | **no** (tested) | no | **yes** |
| ESLint 8 eslintrc | no | no | no | no: dot-folders ignored by default (docs) |
| Oxlint 1.90.0 | yes | **yes** (tested); docs only say `.gitignore`, source has `git_exclude(true)` | no | **no** |
| Oxfmt 0.75.0 | yes | **yes** (tested, docs agree) | no | **no** |
| tsc 7.0.2 | no | no | no | **no** for default `include` (dot-directories skipped, tested); only if `include` names the dir |
| Ruff 0.17.3 | yes | yes (tested) | yes (tested) | **no** |
| Black | root + nested `.gitignore` | **no** (tested) | no (source) | **yes** |
| Biome 2.6.3 | only if `vcs.useIgnoreFile` | only if `vcs.useIgnoreFile` (tested) | only if `vcs.useIgnoreFile` | **yes** (opt-in, default off; scans `.references` by default, tested) |
| Stylelint | no | no | no | **no**: `.` and `**/*.css` do not match dot-directories (tested, with and without the exclude rule); only a file named explicitly is linted |
| mypy | opt-in `exclude_gitignore` | no | no | no: hidden dirs skipped by default |
| pyright / basedpyright / Pylance | no | no | no | no: `**/.*` excluded by default |
| ty | yes | **yes** (tested: listed only when the exclude rule was removed) | yes | no |
| rustfmt / clippy | n/a (module tree / cargo targets) | n/a | n/a | no |
| rust-analyzer | unverified | unverified | unverified | unverified |
| dprint | yes | **yes** (tested: listed only when the exclude rule was removed) | unverified | no |
| markdownlint-cli2 | opt-in `gitignore` (tested: skipped with a `.gitignore` line) | **no** (tested, also with `gitignore: true`) | no | **yes** (scans by default, tested) |
| golangci-lint | n/a (Go package lists) | n/a | n/a | no for `./...` |
| VS Code search / Quick Open | yes | yes | yes | no |
| Zed 1.25.3 | yes | **no** (tested): tree, search and file finder all show it | unverified | **yes**: `file_scan_exclusions` |
| Helix | yes | yes | yes | no |
| Neovim telescope / fzf-lua | via rg/fd: yes | yes | yes | no, unless the `find` fallback is used |
| JetBrains | unverified | unverified | no | **yes** (needs its own exclude) |

Config in `package.json` / `pyproject.toml`: see the last section.

## README claim

README says: "Exclude it yourself in Ruff, Biome, Stylelint and your editor."

- Ruff: **incorrect as a general statement.** `respect-gitignore` defaults to `true` and covers `.ignore`, `.gitignore`, `.git/info/exclude` and global gitignore (https://docs.astral.sh/ruff/settings/ , source comment in https://github.com/astral-sh/ruff/blob/main/crates/ruff_workspace/src/options.rs). Tested with `uvx ruff@0.17.3` (see Ruff). It stays a gap only when the project is not a git repo, `respect-gitignore = false`, or the path is passed explicitly (see `force-exclude` below).
- Biome: correct. Stylelint: **incorrect** for its default file arguments (it never reaches the directory, tested). Prettier, ESLint, Black and markdownlint-cli2 are missing from the sentence and also need a gap note.
- "Your editor": too broad. VS Code search and Quick Open, Helix and ripgrep/fd-based Neovim pickers honor `info/exclude`. Zed does not (tested) and JetBrains needs its own exclude.

## Per-tool

### Prettier
- Ignore: `.prettierignore` (gitignore syntax) and `./.gitignore`; defaults are `.git/.jj/.sl/.svn/.hg` and `node_modules`. "`--ignore-path` ... By default, Prettier looks for `./.gitignore` and `./.prettierignore`." https://prettier.io/docs/ignore , https://prettier.io/docs/cli
- `.git/info/exclude` and `.ignore` are not read. Tested (3.9.10): with `/.references/` only in `info/exclude`, and again with it only in `.ignore`, `prettier --check .` listed `.references/dep/bad.ts` etc. With it in `.gitignore` or `.prettierignore`: 0 hits. Source: file expansion in https://github.com/prettier/prettier/blob/main/src/cli/expand-patterns.js uses fast-glob with only the VCS-dir/node_modules ignorer; ignore files are applied through `createIsIgnoredFunction(ignorePath)` in https://github.com/prettier/prettier/blob/main/src/cli/format.js .
- Exclude: `.prettierignore`, gitignore syntax, relative to the ignore file's directory.
  ```
  # .prettierignore
  /.references/
  ```
- Config detection: `"prettier"` key in `package.json` or `package.yaml` (highest precedence), `.prettierrc*`, `prettier.config.*`. https://prettier.io/docs/configuration

### ESLint
- Flat config (tested 10.13.0): `eslint .` linted `.references/dep/bad.js` with `info/exclude`, and also with `.gitignore` (no gitignore support). Defaults ignore only `**/node_modules/` and `.git/`; dotfiles are no longer ignored in flat config. https://eslint.org/docs/latest/use/configure/ignore , https://eslint.org/docs/latest/use/configure/migration-guide
- Exclude: `globalIgnores()` or a config object with only `ignores`. Patterns are relative to the config file, unlike `.gitignore`: `.config` matches only at the config dir; use `**/.config/` for any depth. Tested: `export default [{ ignores: [".references/"] }, ...]` removed the hit.
  ```js
  // eslint.config.js
  import { defineConfig, globalIgnores } from "eslint/config";
  export default defineConfig([globalIgnores([".references/"])]);
  ```
- Legacy eslintrc: "dot-files (except for `.eslintrc.*`), as well as dot-folders and their contents, are ignored", except when the path to lint is a glob/dir that itself contains a dot-folder. `.eslintignore` (gitignore syntax, cwd only), `ignorePatterns`, or `eslintIgnore` in package.json. https://archive.eslint.org/docs/user-guide/configuring/ignoring-code (v8 docs; no explicit gitignore reading).
- Config detection: legacy supports `eslintConfig` in `package.json`; flat config does not ("it's no longer possible to use a package.json file to configure ESLint"). Flat config removed `.eslintignore` and `--ignore-path` (https://github.com/eslint/eslint/commit/6964cb1e0f073b236cb3288b9d8be495336bbf29).

### Oxlint
- Docs: default ignores are `.git` dirs, minified files, and "files discovered through directory traversal that are matched by `.gitignore` (global gitignore files are not respected)". Hidden files are not ignored. `.eslintignore` supported. https://oxc.rs/docs/guide/usage/linter/ignore-files
- `info/exclude`: not in that page. Source: shared walker config sets `.git_ignore(true)`, `.git_exclude(true)` ("Respect `$GIT_COMMON_DIR/info/exclude`"), `.ignore(false)`, `.git_global(false)`, `.hidden(false)`, `.require_git(has_vcs_boundary)`: https://github.com/oxc-project/oxc/blob/main/crates/oxc_config/src/walk.rs (`configure_walk_builder`). The PR table also lists "nested `.gitignore`, `.git/info/exclude` (during walk)" for oxlint: https://github.com/oxc-project/oxc/pull/25133 . An older oxlint walker (commit 7946ebae) never disabled `git_exclude`, so the `ignore` crate default (on) applied: https://github.com/oxc-project/oxc/blob/7946ebae/apps/oxlint/src/walk.rs . Exact release that introduced the shared config: unverified. Tested with Oxlint 1.90.0 in a git repo whose `.git/info/exclude` held `/.references/`: `oxlint .` reported `src/bad.ts` and `src/bad.js` and nothing under `.references/dep/`.
- Explicit file arguments: per PR #25133 gitignored files passed explicitly are now skipped; before they were always linted (version unverified).
- Exclude: `ignorePatterns`, resolved relative to the config file.
  ```json
  { "ignorePatterns": [".references/**"] }
  ```
  also `--ignore-pattern`. `--no-ignore` does not turn off `.gitignore` filtering during discovery.
- Config files: `.oxlintrc.json` or `oxlint.config.ts`; `package.json` support not mentioned (unverified).

### Oxfmt
- Docs: "Oxfmt respects the same Git ignore rules as Git itself: `.gitignore` files in the current directory tree, in parent directories (up to the repository boundary), `.git/info/exclude`", global gitignore not read; `.prettierignore` supported (global scope); VCS dirs and `node_modules` ignored. https://oxc.rs/docs/guide/usage/formatter/ignore-files
- Source history: in April 2026 oxfmt had its own walker with `.git_exclude(false)` ("Also do not respect `.git/info/exclude`", commit b6abde0, https://github.com/oxc-project/oxc/commit/b6abde0c4d2376ddb769441233341fa23b1b413c); it now uses the shared `configure_walk_builder` with `git_exclude(true)` (link above). So older oxfmt builds may not honor it; the first version that does is unverified. Tested with Oxfmt 0.75.0, same layout: `oxfmt --check .` listed `src/bad.js`, `src/bad.ts` and `tsconfig.json`, nothing under `.references/`.
- Exclude: `ignorePatterns` (gitignore syntax, rooted at the config file's directory; patterns containing `..` rejected). https://oxc.rs/docs/guide/usage/formatter/config-file-reference
  ```json
  { "ignorePatterns": [".references/**"] }
  ```
- Config files: `.oxfmtrc.json`, `.oxfmtrc.jsonc`, `oxfmt.config.ts`, `oxfmt.config.mts` (nearest wins). Prettier's `package.json` key is only read by `oxfmt --migrate prettier`. https://oxc.rs/docs/guide/usage/formatter/config.html , https://oxc.rs/docs/guide/usage/formatter/migrate-from-prettier.html

### tsc / tsserver
- tsconfig `exclude` default: `node_modules`, `bower_components`, `jspm_packages`, `outDir`; it only filters what `include` finds, and imports/`files` can still pull excluded files in. Default `include` is `**/*`; paths are relative to the tsconfig. Syntax: `*`, `?`, `**/`; a last segment without extension is a directory. https://www.typescriptlang.org/tsconfig/
- Dot-directories: the `**` wildcard "does not match subdirectories that start with a . character" (comment in `utilities.ts`, quoted in https://github.com/sveltejs/language-tools/issues/1861 at TypeScript commit 1dc5b28; `doubleAsteriskRegexFragment: (/${implicitExcludePathRegexPattern}[^/.][^/]*)*?`). So `.references` is skipped by the default `**/*` include. Does not read `.gitignore`/`info/exclude`. Tested with TypeScript 7.0.2 (native) and `{"compilerOptions":{"strict":true,"noEmit":true,"allowJs":true}}` (default `include`): `tsc --listFilesOnly -p .` listed `src/bad.ts` and not `.references/dep/bad.ts`. With `"include": [".references/**/*", "src/**/*"]` it listed `.references/dep/bad.ts`. (With `allowJs`, `src/bad.js` was not listed in either run; not investigated, irrelevant to `.references`.)
- Gap remains if `include` names the dir (e.g. `"include": [".references/**"]`) or code imports from it.
  ```json
  { "exclude": [".references"] }
  ```
- Config detection: `tsconfig.json` only.

### Ruff
- Tested with `uvx ruff@0.17.3 check --output-format concise .` (matrix, one source at a time, `.references/dep/bad.py` hits): none = 1; `.gitignore` = 0; `.ignore` = 0; `.git/info/exclude` = 0; `info/exclude` + `--no-respect-gitignore` = 1 (so it is the gitignore machinery); the same layout in a directory without `.git` = 1 (scanned). `ruff format --check .` also skipped it.
- Docs: `exclude` default list (no `.references`), `extend-exclude` adds, `force-exclude` applies excludes even to explicitly passed paths. Single-path patterns like `.mypy_cache` match any directory with that name; patterns with a slash are relative to the project root; globset syntax. https://docs.astral.sh/ruff/settings/
  ```toml
  [tool.ruff]
  extend-exclude = [".references"]
  ```
  (docs syntax, not needed for the git case.) Editors pass files explicitly, so use `force-exclude = true` for editor integrations (docs text truncated in my fetch; exact editor interaction unverified).
- Config detection: `pyproject.toml` (needs a `[tool.ruff]` section, otherwise skipped), `ruff.toml`, `.ruff.toml`; closest wins, `.ruff.toml` > `ruff.toml` > `pyproject.toml`. https://docs.astral.sh/ruff/configuration/

### Black
- Docs: "If `--exclude` is not set, Black will automatically ignore files and directories in `.gitignore` file(s)"; setting `--exclude` replaces all default exclusions (including the gitignore use). https://black.readthedocs.io/en/stable/usage_and_configuration/file_collection_and_discovery.html , https://github.com/psf/black/blob/main/docs/usage_and_configuration/the_basics.md
- Source: `get_gitignore(root)` opens `root / ".gitignore"` only; `gen_python_files` accumulates it per directory. `info/exclude` and `.ignore` are never read. https://github.com/psf/black/blob/main/src/black/files.py . Black 26.5.1 per docs page title.
- **Run (third pass): `black --check .` listed `.references/x/bad.py` with only the exclude rule in place.** The source reading holds.
- Exclude (regex, forward slashes, `^/` anchors to project root):
  ```toml
  [tool.black]
  extend-exclude = '^/\.references/'
  ```
  `--force-exclude` also applies to explicitly passed files (editors, pre-commit).
- **Fifth pass (2026-10-28), Black 26.10.1, scratch repo under `~/.cache` (deleted), only `/.references/` in `.git/info/exclude`, `black --check`; `src/bad.py` was reported in every case.** Whether `.references/x/bad.py` was reported:

  | Setting | Reported |
  |---|---|
  | none | yes |
  | `exclude`, `extend-exclude` or `force-exclude` naming the directory (`^/\.references/`, `\.references`, `.references`) | no, all nine |
  | `extend-exclude = '^/other/\|^/\.references/'` | no |
  | `exclude = '^/nothing/'` and `/.references/` in `.gitignore` | **yes** |
  | `/.references/` in `.gitignore`, no options | no |
  | `/.references/` in `.gitignore`, `extend-exclude = '^/other/'` | no |
  | explicit file arguments | only `force-exclude` hides it |

  So all three options cover; `exclude` replaces the `.gitignore` handling and `extend-exclude` keeps it; `force-exclude` is the only one that also covers files passed explicitly (editors, pre-commit). Not run: other Black versions, a nested `.references`, Windows paths, pre-commit or editor runs.
- Config: `[tool.black]` in `pyproject.toml`.

### Biome
- Opt-in only: `vcs.enabled` default `false`, `vcs.useIgnoreFile` default `false`. When on, Biome applies `.gitignore`, `.git/info/exclude` (from the common git dir in linked worktrees) and `.ignore`, including nested files. https://biomejs.dev/reference/configuration/ , https://biomejs.dev/configuration/integrate-with-vcs/
- Exclude: `files.includes` with a leading `**` then a negation; `!` excludes from lint/format, `!!` also from the scanner/indexing. Paths resolve from the config file's directory. Dir exclusion with bare `!dir` works only in `files.includes`; in `linter.includes`/`formatter.includes` use `!dir/**` (website source, https://github.com/biomejs/website/blob/main/src/content/docs/reference/configuration.mdx).
  ```json
  { "files": { "includes": ["**", "!!.references"] } }
  ```
  (derived from the documented `!!**/dist` example; not run). `node_modules` is always ignored.
- Tested with Biome 2.6.3, `/.references/` only in `.git/info/exclude`: `biome check .` with no config reported `.references/dep/bad.ts` and `bad.js` (8 errors in total, both directories); after adding `{"vcs":{"enabled":true,"clientKind":"git","useIgnoreFile":true}}` it reported only `src/*`, so `info/exclude` is what excluded it (`.gitignore` was absent). Whether `vcs.root` matters in a monorepo: unverified.
- Fourth pass (2026-10-28, Biome 2.6.3 again, same scratch repo, deleted afterwards): `files.includes: ["**", "!!.references"]` hides `.references/x/bad.js` (as does `"!.references"`; `"!!.references/**"` also, but Biome's `useBiomeIgnoreFolder` lint flags it, so the bare name is the snippet). `vcs` with `enabled: true`, `clientKind: "git"` and `useIgnoreFile: true` hides it. **`useIgnoreFile: true` with `enabled` unset, or alone, does not**: the directory is scanned. `enabled: true` without `clientKind` is a config error.
- Config: `biome.json`, `biome.jsonc`, `.biome.json`, `.biome.jsonc`; no `package.json` config. https://biomejs.dev/configuration/configure-biome/

### Stylelint
- No gitignore handling: ignores via `.stylelintignore` (gitignore syntax, relative to cwd, read from cwd only), `ignoreFiles` in config, `--ignore-path`/`--ignore-pattern`. `node_modules` ignored by default, but "this is overridden if `ignoreFiles` is set". `--ignore-path .gitignore` is the documented way to reuse `.gitignore`. https://github.com/stylelint/stylelint/blob/main/docs/user-guide/ignore-code.md , https://github.com/stylelint/stylelint/blob/main/docs/user-guide/configure.md .
- **Run (third pass): it does not reach `.references/` by default.** `stylelint .` and `stylelint "**/*.css"` linted `src/` and never `.references/x/bad.css`, with `/.references/` in `info/exclude` and with the file empty; naming `.references/x/bad.css` lints it. File globs do not match dot-directories. So it is in the same class as tsc with the default `include`: not reported, and a script that globs the directory explicitly is outside what we detect. Its docs say nothing of this, and the earlier "yes" in the table came from them.
- Exclude:
  ```js
  // stylelint.config.js
  export default { ignoreFiles: [".references/**", "node_modules/**"] };
  ```
  (`ignoreFiles` globs are relative to the config file when it was found on disk.) Or a `.stylelintignore` with `/.references/`.
- Config: `stylelint.config.js` etc.; `.stylelintrc*` and a `stylelint` property in `package.json` are "currently supported ... but we may remove these in the future".

### mypy
- `exclude` regex applies to recursive discovery only; "mypy will never recursively discover files and directories named site-packages, node_modules or __pycache__, or those whose name starts with a period". So `.references` is skipped when discovered recursively; imports can still reach it. `exclude_gitignore` (default `False`) adds `.gitignore` matches; no `info/exclude`. https://mypy.readthedocs.io/en/stable/command_line.html , https://mypy.readthedocs.io/en/stable/config_file.html . Version where `exclude_gitignore` appeared: unverified.
  ```toml
  [tool.mypy]
  exclude = ['^\.references/']
  ```
- Config: `mypy.ini`, `.mypy.ini`, `pyproject.toml` `[tool.mypy]`, `setup.cfg` `[mypy]`, first found walking up.

### pyright / basedpyright / Pylance
- `exclude` defaults: `**/node_modules`, `**/__pycache__`, `**/.*` (hidden directories), editable shadow dirs, detected venvs; user entries are added on top; `useDefaultExcludes` turns it off. Does not read `.gitignore` ("We don't have any plans for pyright to read the `.gitignore` file", https://github.com/microsoft/pyright/issues/129). https://github.com/microsoft/pyright/blob/main/docs/configuration.md . basedpyright's older docs list the same defaults: https://docs.basedpyright.com/latest/configuration/config-files/ ; any basedpyright-specific gitignore option: unverified.
- Pylance `python.analysis.exclude` replaces defaults when set (dot-directories are default-excluded); `python.analysis.useDefaultExcludes` (default true) keeps them. https://github.com/microsoft/pylance-release/blob/main/docs/settings/python_analysis_exclude.md , https://github.com/microsoft/pylance-release/blob/main/docs/settings/python_analysis_useDefaultExcludes.md
  ```toml
  [tool.pyright]
  exclude = ["**/.references"]
  ```
  Files imported by non-excluded files are still analyzed.
- Config: `pyrightconfig.json` (wins) or `[tool.pyright]` in `pyproject.toml`.

### ty
- `respect-ignore-files` (default `true`) excludes files ignored by `.ignore`, `.gitignore`, `.git/info/exclude`, global gitignore. https://docs.astral.sh/ty/reference/configuration/ , https://docs.astral.sh/ty/exclusions/ . **Run (third pass):** `ty check .` skipped `.references/x/bad.py` (a type error, shown by naming the file) and reported it once the exclude rule was removed, so it honors `info/exclude`.
- Exclude: `src.exclude`, gitignore-like, anchored to project root (`**/x` for any depth), `./src/` dir-only, `!pattern` negates.
  ```toml
  [tool.ty.src]
  exclude = [".references/"]
  ```
- Config: `ty.toml` (wins) or `[tool.ty]` in `pyproject.toml`, nearest parent.

### rustfmt, clippy, rust-analyzer
- rustfmt/`cargo fmt`: walks the module tree (confirmed locally by the task owner). Clippy runs on cargo targets; I did not find a source and did not test (unverified, expected no scan).
- rust-analyzer: `rust-analyzer.files.exclude` (default `[]`): "relative to the workspace root, and globs are not supported"; may also need VS Code `files.watcherExclude`; `files.watcher` is `client` by default. https://rust-analyzer.github.io/book/configuration.html , rename from `excludeDirs`: https://github.com/rust-lang/rust-analyzer/pull/18998 . Whether it honors gitignore/`info/exclude`: unverified. Settings come from the editor; `rust-analyzer.toml` is partial.
  ```json
  { "rust-analyzer.files.exclude": [".references"] }
  ```

### dprint
- "By default, dprint respects `.gitignore` files (as well as a repository's `.git/info/exclude` file)"; global gitignore opt-in via `DPRINT_GLOBAL_GITIGNORE=1`; `--no-gitignore` disables. `.ignore` not mentioned (unverified). https://dprint.dev/cli/ , https://dprint.dev/config/ . **Run (third pass, dprint 0.64.3 with the TypeScript plugin):** `dprint check` skipped `.references/x/bad.ts`, and flagged it once the exclude rule was removed.
- Exclude: `excludes` (gitignore extended glob syntax) in `dprint.json`; `node_modules` excluded by default.
  ```json
  { "excludes": [".references"] }
  ```
- Config: `dprint.json`, `.dprint.json`, `dprint.jsonc`, `.dprint.jsonc` (https://docs.rs/dprint-config/latest/dprint_config/struct.DprintConfig.html).

### markdownlint
- markdownlint-cli2: `gitignore` option (Boolean or glob) "to automatically ignore files referenced by `.gitignore` (or similar)"; `true` uses `.gitignore` files in the tree and up to the repo root. Default is off. **Run (third pass, 0.24.0):** with no config it linted `.references/x/bad.md`; with `"gitignore": true` and only `info/exclude` it still did; with `"gitignore": true` and a `.gitignore` line for the directory it did not. So `info/exclude` is not read, and the gap is real. `ignores` array (negated and appended to globs). https://github.com/DavidAnson/markdownlint-cli2
  ```jsonc
  // .markdownlint-cli2.jsonc
  { "ignores": [".references/**"] }
  ```
- Config files: `.markdownlint-cli2.{jsonc,yaml,cjs,mjs}`; `package.json` support and markdownlint-cli (`.markdownlintignore`): unverified.

### golangci-lint
- Go package patterns with `...` exclude `vendor`, `testdata`, and names beginning with `_` or `.`, so `golangci-lint run ./...` never enters `.references`. https://go.dev/src/cmd/go/internal/help/helpdoc.go (`HelpPackages`). Whether golangci-lint itself reads `.gitignore`: unverified.
- Exclude (v2): `linters.exclusions.paths` and `formatters.exclusions.paths` (regex-style paths). https://golangci-lint.run/docs/product/migration-guide/ , https://golangci-lint.run/docs/linters/false-positives/
  ```yaml
  linters:
    exclusions:
      paths:
        - \.references
  ```
- Config file names: unverified in fetched text.

## Editors

### VS Code
- Search and Quick Open ignore by `search.exclude`, `files.exclude`, `.gitignore`; "Besides `.gitignore`, we also look at the `.ignore`, `.rgignore`, and `.git/info/exclude` files" (ripgrep logic). `"search.useIgnoreFiles": false` disables. https://github.com/microsoft/vscode/wiki/Search-Issues . The proposed API also has `search.useParentIgnoreFiles` and `search.useGlobalIgnoreFiles` (https://github.com/microsoft/vscode/blob/main/src/vscode-dts/vscode.proposed.findTextInFiles2.d.ts). Settings defaults: unverified.
- Explorer: `files.exclude` hides; `.gitignore` is applied in the explorer only via `explorer.excludeGitIgnore` (named in https://code.visualstudio.com/docs/configure/settings; default and `info/exclude` behavior: unverified).
- Glob syntax: in settings you must write `**/example` for a nested folder (the Search view assumes `**/`); patterns are relative to the workspace folder. https://code.visualstudio.com/docs/editor/glob-patterns
  ```jsonc
  // .vscode/settings.json (path not in the fetched excerpt)
  {
    "files.exclude": { "**/.references": true },
    "search.exclude": { "**/.references": true },
    "files.watcherExclude": { "**/.references/**": true }
  }
  ```
  `files.watcherExclude` default value: unverified.
- Python: Pylance default-excludes dot directories (see pyright). tsserver follows tsconfig as tsc; loose-file behavior unverified.

### Zed
- `file_scan_exclusions` (default: `**/.git`, `**/.svn`, `**/.hg`, `**/.jj`, `**/.sl`, `**/.repo`, `**/CVS`, `**/.DS_Store`, `**/Thumbs.db`, `**/.classpath`, `**/.settings`): "Exclude files matching these glob patterns from file scans, file searches, and the project file tree." In project settings it extends the user list; `"..."` inserts the inherited defaults. https://zed.dev/docs/reference/all-settings . `file_scan_inclusions` (default `[".env*"]`) re-includes git-ignored files; exclusions win. Project settings file path: `.zed/settings.json`, confirmed by the hand test on Zed 1.25.3 (the setting was put there and took effect); not in the docs I fetched.
  ```json
  { "file_scan_exclusions": ["**/.references", "..."] }
  ```
- `info/exclude`: **tested by hand on Zed 1.25.3 (Windows, 2026-10-23), default settings, `/.references/` only in `.git/info/exclude`: the project panel lists `.references/`, project search finds the string in `.references/dep/`, and the file finder lists it. `info/exclude` did nothing for any of the three.** With `file_scan_exclusions` set, all three stopped showing it.
  - This contradicts what the source suggests: Zed's worktree has a `RepoExclude` ignore layer (merged: https://github.com/zed-industries/zed/pull/51536, respect `.git/info/exclude` in linked worktrees; https://github.com/zed-industries/zed/pull/62325, anchoring). The layer exists, but it did not take effect in this test. Why (platform, version, or a different code path such as the project panel and search not applying it) is unverified; only Windows 1.25.3 was tried, no Linux or macOS run and no `.gitignore` control run. Treat `info/exclude` as not reliable in Zed, as the maintainer's earlier experience also suggested.

### Helix
- `[editor.file-picker]` defaults: `hidden = true`, `ignore = true` (`.ignore`), `git-ignore = true`, `git-global = true`, `git-exclude = true` ("Enables reading `.git/info/exclude` files"), `parents = true`; git options only apply in a git repo. Extra: `.helix/ignore` in the workspace, or `~/.config/helix/ignore`. https://docs.helix-editor.com/editor.html
  ```
  # .helix/ignore (or .ignore)
  .references/
  ```
  Project config at `.helix/config.toml` (https://docs.helix-editor.com/configuration.html).

### Neovim
- telescope `find_files` runs `rg --files --color never` if `rg` exists, else `fd`/`fdfind`, else `find`; `hidden`, `no_ignore`, `no_ignore_parent` options map to `--hidden`, `--no-ignore`. https://github.com/nvim-telescope/telescope.nvim/blob/master/lua/telescope/builtin/__files.lua . With rg/fd, `info/exclude` is honored (confirmed locally by the task owner). The `find` fallback ignores it (it only filters `*/.*` when `hidden` is off).
- fzf-lua `files` prefers `fdfind`/`fd`, then `rg`, then `find`/`dir`; `--no-ignore` is a toggle. https://github.com/ibhagwan/fzf-lua/blob/d39620ba/lua/fzf-lua/providers/files.lua
- Live grep uses rg (unverified here, same ignore rules apply).

### JetBrains (checking `docs/private/jetbrains.md`)
Verified against JetBrains docs:
- Mark Directory As > Excluded exists; "Files in excluded folders are ignored by code completion, navigation and inspection." https://www.jetbrains.com/help/idea/content-root.html . Search skips excluded folders (Stack Overflow only, not a primary source), though the Search Everywhere "All" tab can still show them via Recent files.
- Settings > Editor > File Types > "Ignore files and folders": a semicolon-separated list with wildcards; defaults like `*.pyc;*.pyo;...;.git;.hg;.svn;CVS;__pycache__;`. https://www.jetbrains.com/help/phpstorm/creating-and-registering-file-types.html (PhpStorm page; the IDEA equivalent was not fetched; in newer UIs it is an "Ignored Files and Folders" tab). Files matched there are hidden from the Project tree as well (Stack Overflow, secondary).
- Scope language: `file:dir//*` recursive, `!` NOT, `&&`/`||`, module modifier `file[module]:path`, and the docs example `!file:*/.npm//*` "exclude all .npm folders". https://www.jetbrains.com/help/idea/scope-language-syntax-reference.html

Problems in the Gemini note:
- "By default, excluded folders will vanish from the project tree" is not supported: the docs say excluded folders are marked with an icon.
- Corrupted `.iml` line: `<orderEntry type="sourceFolder" source-dir="`" />` is not valid; irrelevant to the exclusion. A normal `.iml` has `<orderEntry type="sourceFolder" forTests="false" />` (from memory, unverified). The `<excludeFolder url="file://$MODULE_DIR$/.references" />` element is the standard form but I found no JetBrains page documenting the `.iml` format: unverified.
- Two conflicting scope patterns: `!file:.references//*` (Option 2) is a bare negation, and the XML example `file:[*]:*&&!file[root]:.references//* ` (trailing space, `file[root]` only matches a module named `root`, `[*]` is not a documented form). Per the syntax page the first is the right shape; a scope that means "everything except `.references`" would be `file:*//*&&!file:.references//*` (unverified that `file:*//*` matches all project files). The `.idea/scopes/*.xml` file with `DependencyValidationManager` is also not documented in what I fetched: unverified.
- "JetBrains still indexes git-ignored files by default" and `Project Files` scope behavior: unverified. JetBrains staff confirmed `.git/info/exclude` is a supported place for local ignores since 2019.2 (https://intellij-support.jetbrains.com/hc/en-us/community/posts/360005958220-Intellij-ignore-feature-gone), which affects VCS status, not necessarily indexing or search. A grey-out bug for `info/exclude` entries was reported in the third-party IdeaGitignore plugin (https://github.com/JetBrains/idea-gitignore/issues/325).
- Conclusion: a gap note is needed; recommend Mark Directory As > Excluded (stored per project, in the module `.iml`/`.idea`) rather than `.gitignore` handling.

## Config detection in `package.json` / `pyproject.toml`

| Tool | `package.json` | `pyproject.toml` | Dedicated file |
|---|---|---|---|
| Prettier | `"prettier"` key | n/a | `.prettierrc*`, `prettier.config.*` |
| ESLint flat | no | n/a | `eslint.config.*` |
| ESLint legacy | `eslintConfig`, `eslintIgnore` | n/a | `.eslintrc.*` |
| Oxlint / Oxfmt | no (Oxfmt only migrates it) | n/a | `.oxlintrc.json`, `oxlint.config.ts`, `.oxfmtrc.json(c)`, `oxfmt.config.(m)ts` |
| Biome | no | n/a | `biome.json(c)`, `.biome.json(c)` |
| Stylelint | `stylelint` (legacy, may be removed) | n/a | `stylelint.config.*`, `.stylelintrc*` |
| tsc | no | n/a | `tsconfig.json` |
| Ruff | n/a | `[tool.ruff]` | `ruff.toml`, `.ruff.toml` |
| Black | n/a | `[tool.black]` | none |
| mypy | n/a | `[tool.mypy]` | `mypy.ini`, `.mypy.ini`, `setup.cfg` |
| pyright | n/a | `[tool.pyright]` | `pyrightconfig.json` |
| ty | n/a | `[tool.ty]` | `ty.toml` |
| dprint | unverified | n/a | `dprint.json(c)`, `.dprint.json(c)` |

## Unverified

- Oxlint and Oxfmt: the first release honoring `info/exclude` (Oxfmt) and whether `package.json` config is read (Oxlint).
- Prettier: reading `.gitignore` for a Project below the directory of the Prettier config (docs only; refs counts the `.gitignore` of the config's own directory).
- Biome: `vcs.root` in monorepos; a `vcs` section inherited through `extends`.
- markdownlint-cli2: `gitignore` given as a glob string (only `true` was run); it reads `.gitignore`, so it is no cover while refs writes only `info/exclude`.
- mypy, pyright: docs/source only, no local run.
- Ruff `force-exclude` editor interaction (docs text truncated in my fetch).
- rust-analyzer gitignore handling; clippy source; golangci-lint gitignore handling and config file names; markdownlint-cli2 `package.json` support; markdownlint-cli.
- VS Code: defaults of `search.useIgnoreFiles`, `explorer.excludeGitIgnore`, `files.watcherExclude`; workspace settings path in docs; tsserver loose-file behavior.
- Zed: why `info/exclude` had no effect in 1.25.3 on Windows (other platforms, a `.gitignore` control run, and linked worktrees not tried).
- JetBrains: `.iml` format, scope XML file format, `file:*//*`, indexing of git-ignored files, IDEA-specific File Types page.
- mypy: release that added `exclude_gitignore`. Black: release that added gitignore support. Black: other versions, a nested `.references`, Windows paths and pre-commit or editor runs (fifth pass).
