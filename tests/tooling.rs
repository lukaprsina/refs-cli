//! Which tools leave the references directory in reach, over real directories.

use std::fs;
use std::path::{Path, PathBuf};

mod common;

use refs_cli::tooling::{Gap, gaps};
use tempfile::TempDir;

const DIR: &str = ".references";

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap()
}

/// A git worktree with `apps/web` in it: the worktree top and the project directory.
fn worktree() -> (TempDir, PathBuf, PathBuf) {
    let repo = TempDir::new().unwrap();
    common::git(repo.path(), &["init", "-q"]);
    let top = canonical(repo.path());
    let project = top.join("apps/web");
    fs::create_dir_all(&project).unwrap();
    (repo, top, project)
}

fn gap(tool: &'static str, file: &str, snippet: &str) -> Gap {
    Gap {
        tool,
        file: file.into(),
        snippet: snippet.into(),
        create: false,
    }
}

/// A gap whose file does not exist yet: the note says to create it.
fn to_create(tool: &'static str, file: &str, snippet: &str) -> Gap {
    Gap {
        create: true,
        ..gap(tool, file, snippet)
    }
}

fn tools(found: &[Gap]) -> Vec<&'static str> {
    found.iter().map(|gap| gap.tool).collect()
}

#[test]
fn legacy_eslint_is_covered_whatever_its_files_say() {
    // ESLint 8 ignores dot-folders by default.
    for files in [
        &[".eslintrc.json"][..],
        &[".eslintrc.json", ".eslintignore"],
        &[".eslintignore"],
    ] {
        let dir = TempDir::new().unwrap();
        for file in files {
            fs::write(dir.path().join(file), "dist\n").unwrap();
        }

        assert_eq!(gaps(dir.path(), DIR, &[]), [], "{files:?}");
    }
}

#[test]
fn legacy_eslint_with_a_dependency_is_still_covered() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".eslintrc.json"), "{}").unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies": {"eslint": "^8"}}"#,
    )
    .unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn a_flat_config_wins_over_the_legacy_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("eslint.config.mjs"), "export default [];").unwrap();
    fs::write(dir.path().join(".eslintrc.json"), "{}").unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [gap(
            "eslint",
            "eslint.config.mjs",
            r#"{ ignores: [".references/**"] }"#
        )]
    );
}

#[test]
fn a_flat_config_is_not_covered_by_eslintignore_which_eslint_then_ignores() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("eslint.config.js"), "export default [];").unwrap();
    fs::write(dir.path().join(".eslintignore"), ".references/\n").unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [gap(
            "eslint",
            "eslint.config.js",
            r#"{ ignores: [".references/**"] }"#
        )]
    );
}

#[test]
fn prettier_is_fixed_in_prettierignore_even_with_a_config() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [to_create("prettier", ".prettierignore", ".references/")]
    );
}

#[test]
fn a_prettierignore_alone_counts_as_using_prettier() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".prettierignore"), "dist\n").unwrap();

    assert_eq!(tools(&gaps(dir.path(), DIR, &[])), ["prettier"]);
}

#[test]
fn prettier_is_covered_by_the_directory_in_prettierignore() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
    fs::write(dir.path().join(".prettierignore"), ".references\n").unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn oxlint_and_tsc_are_not_reported() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".oxlintrc.json"), "{}").unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn tsconfig_variants_are_not_detected() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("tsconfig.build.json"), "{}").unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn a_config_further_up_is_found_with_the_path_from_its_directory() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("eslint.config.js"), "").unwrap();

    assert_eq!(
        gaps(&project, DIR, &[]),
        [gap(
            "eslint",
            "../../eslint.config.js",
            r#"{ ignores: ["apps/web/.references/**"] }"#
        )]
    );
}

#[test]
fn the_nearest_config_of_a_tool_wins_even_when_it_is_covered() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("eslint.config.js"), "").unwrap();
    fs::write(project.join("eslint.config.js"), ".references").unwrap();

    assert_eq!(gaps(&project, DIR, &[]), []);
}

#[test]
fn each_tool_takes_its_own_nearest_directory() {
    let (_repo, top, project) = worktree();
    fs::write(top.join(".prettierrc"), "{}").unwrap();
    fs::write(project.join("eslint.config.js"), "").unwrap();

    let mut found = tools(&gaps(&project, DIR, &[]));
    found.sort();
    assert_eq!(found, ["eslint", "prettier"]);
}

#[test]
fn a_config_above_the_worktree_is_not_read() {
    let outer = TempDir::new().unwrap();
    fs::write(outer.path().join("eslint.config.js"), "").unwrap();
    let repo = outer.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    common::git(&repo, &["init", "-q"]);

    assert_eq!(gaps(&canonical(&repo), DIR, &[]), []);
}

#[test]
fn outside_a_worktree_only_the_project_directory_is_read() {
    let outer = TempDir::new().unwrap();
    fs::write(outer.path().join("eslint.config.js"), "").unwrap();
    let project = outer.path().join("project");
    fs::create_dir_all(&project).unwrap();

    assert_eq!(gaps(&project, DIR, &[]), []);
}

#[test]
fn biome_is_fixed_in_its_config() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("biome.jsonc"), "{}").unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [gap(
            "biome",
            "biome.jsonc",
            r#""files": { "includes": ["**", "!!.references"] }"#
        )]
    );
}

#[test]
fn biome_is_covered_by_the_directory_in_its_config() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("biome.json"),
        r#"{"files":{"includes":["**","!!.references"]}}"#,
    )
    .unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

/// A directory with a `biome.jsonc` of this text.
fn biome(config: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("biome.jsonc"), config).unwrap();
    dir
}

#[test]
fn biome_is_covered_by_reading_the_vcs_ignore_files() {
    let dir = biome(
        r#"{
            // Biome reads .git/info/exclude through this
            "vcs": { "enabled": true, "clientKind": "git", "useIgnoreFile": true, },
        }"#,
    );

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn biome_vcs_needs_both_enabled_and_use_ignore_file() {
    for vcs in [
        r#""vcs": { "clientKind": "git", "useIgnoreFile": true }"#,
        r#""vcs": { "enabled": true, "clientKind": "git" }"#,
        r#""vcs": { "enabled": false, "clientKind": "git", "useIgnoreFile": true }"#,
        r#""linter": { "enabled": true }, "vcs": { "useIgnoreFile": true }"#,
    ] {
        let dir = biome(&format!("{{ {vcs} }}"));

        assert_eq!(tools(&gaps(dir.path(), DIR, &[])), ["biome"], "{vcs}");
    }
}

#[test]
fn biome_is_covered_only_by_a_negated_entry() {
    for (includes, covered) in [
        (r#"["**", "!.references"]"#, true),
        (r#"["**", "!!.references"]"#, true),
        (r#"["**", "!**/.references"]"#, true),
        (r#"["src", ".references"]"#, false),
        (r#"[".references/**"]"#, false),
    ] {
        let dir = biome(&format!(r#"{{ "files": {{ "includes": {includes} }} }}"#));

        let expected: &[&str] = if covered { &[] } else { &["biome"] };
        assert_eq!(tools(&gaps(dir.path(), DIR, &[])), expected, "{includes}");
    }
}

#[test]
fn biome_vcs_in_a_comment_or_a_broken_config_is_not_cover() {
    for config in [
        r#"{ // "vcs": { "enabled": true, "useIgnoreFile": true }
        }"#,
        r#"{ "vcs": { "enabled": true, "useIgnoreFile": true "#,
        r#"{ // "files": { "includes": ["**", "!!.references"] }
        }"#,
        r#"{ "files": { "includes": ["**", "!!.references"] "#,
    ] {
        let dir = biome(config);

        assert_eq!(tools(&gaps(dir.path(), DIR, &[])), ["biome"], "{config}");
    }
}

#[test]
fn markdownlint_takes_its_ignores_in_the_format_of_its_config() {
    let json = TempDir::new().unwrap();
    fs::write(json.path().join(".markdownlint-cli2.jsonc"), "{}").unwrap();
    let yaml = TempDir::new().unwrap();
    fs::write(yaml.path().join(".markdownlint-cli2.yaml"), "{}").unwrap();

    assert_eq!(
        gaps(json.path(), DIR, &[]),
        [gap(
            "markdownlint",
            ".markdownlint-cli2.jsonc",
            r#""ignores": [".references/**"]"#
        )]
    );
    assert_eq!(
        gaps(yaml.path(), DIR, &[]),
        [gap(
            "markdownlint",
            ".markdownlint-cli2.yaml",
            "ignores:\n  - \".references/**\""
        )]
    );
}

#[test]
fn zed_keeps_its_default_exclusions() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".zed")).unwrap();
    fs::write(dir.path().join(".zed/settings.json"), "{}").unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [gap(
            "zed",
            ".zed/settings.json",
            r#""file_scan_exclusions": ["**/.references", "..."]"#
        )]
    );
}

#[test]
fn zed_is_covered_by_the_directory_in_its_settings() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".zed")).unwrap();
    fs::write(
        dir.path().join(".zed/settings.json"),
        r#"{"file_scan_exclusions": ["**/.references", "..."]}"#,
    )
    .unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

/// A directory with a `.zed/settings.json` of this text.
fn zed(settings: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".zed")).unwrap();
    fs::write(dir.path().join(".zed/settings.json"), settings).unwrap();
    dir
}

#[test]
fn zed_is_covered_by_any_entry_naming_the_directory() {
    for entry in [
        r#""**/.references""#,
        r#""/home/me/code/app/.references""#,
        r#"".references/**""#,
    ] {
        let dir = zed(&format!(
            "{{ // generated\n \"file_scan_exclusions\": [\"**/.git\", {entry}, \"...\",], }}"
        ));

        assert_eq!(gaps(dir.path(), DIR, &[]), [], "{entry}");
    }
}

#[test]
fn zed_is_not_covered_by_the_directory_named_in_another_setting() {
    for settings in [
        r#"{ "terminal": { "working_directory": { "always": { "directory": ".references" } } } }"#,
        r#"{ "file_scan_inclusions": [".references"] }"#,
        r#"{ // "file_scan_exclusions": ["**/.references"]
        }"#,
        r#"{ "file_scan_exclusions": "**/.references" }"#,
    ] {
        let dir = zed(settings);

        assert_eq!(tools(&gaps(dir.path(), DIR, &[])), ["zed"], "{settings}");
    }
}

#[test]
fn a_zed_settings_file_further_up_names_the_directory_from_the_top() {
    let (_repo, top, project) = worktree();
    fs::create_dir(top.join(".zed")).unwrap();
    fs::write(top.join(".zed/settings.json"), "{}").unwrap();

    assert_eq!(
        gaps(&project, DIR, &[]),
        [gap(
            "zed",
            "../../.zed/settings.json",
            r#""file_scan_exclusions": ["**/apps/web/.references", "..."]"#
        )]
    );
}

const BLACK_LINE: &str = "extend-exclude = '^/\\.references/'";
const BLACK_FULL: &str = "[tool.black]\nextend-exclude = '^/\\.references/'";

#[test]
fn black_is_in_use_with_its_table_in_pyproject() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.black]\nline-length = 100\n",
    )
    .unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [gap("black", "pyproject.toml", BLACK_LINE)]
    );
}

#[test]
fn a_pyproject_without_the_table_is_not_black() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.ruff]\nline-length = 100\n",
    )
    .unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn a_pyproject_that_does_not_parse_says_nothing() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("pyproject.toml"), "[tool.black\n").unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn black_is_covered_by_the_directory_in_pyproject() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.black]\nextend-exclude = '^/\\.references/'\n",
    )
    .unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn black_gets_the_table_header_only_when_pyproject_lacks_it() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".pre-commit-config.yaml"),
        "repos:\n  - repo: https://github.com/psf/black\n    hooks:\n      - id: black\n",
    )
    .unwrap();
    fs::write(dir.path().join("pyproject.toml"), "[tool.ruff]\n").unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [gap(
            "black",
            "pyproject.toml",
            "[tool.black]\nextend-exclude = '^/\\.references/'"
        )]
    );
}

#[test]
fn black_is_covered_by_any_of_its_exclude_options() {
    for option in ["exclude", "extend-exclude", "force-exclude"] {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            format!("[tool.black]\n{option} = '/(build|\\.references)/'\n"),
        )
        .unwrap();

        assert_eq!(gaps(dir.path(), DIR, &[]), [], "{option}");
    }
}

#[test]
fn black_is_not_covered_by_the_directory_named_for_another_tool() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.black]\nline-length = 100\n[tool.pytest.ini_options]\nnorecursedirs = ['.references']\n",
    )
    .unwrap();

    assert_eq!(tools(&gaps(dir.path(), DIR, &[])), ["black"]);
}

#[test]
fn black_is_covered_by_the_hook_excluding_the_directory() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".pre-commit-config.yaml"),
        "repos:\n  - repo: x\n    hooks:\n      - id: black\n        exclude: ^\\.references/\n",
    )
    .unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn a_nested_references_directory_is_anchored_in_the_black_regex() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("pyproject.toml"), "[tool.black]\n").unwrap();

    assert_eq!(
        gaps(&project, DIR, &[]),
        [gap(
            "black",
            "../../pyproject.toml",
            "extend-exclude = '^/apps/web/\\.references/'"
        )]
    );
}

#[test]
fn a_package_json_key_or_dependency_puts_a_tool_in_use() {
    let key = TempDir::new().unwrap();
    fs::write(
        key.path().join("package.json"),
        r#"{"prettier": {"semi": false}}"#,
    )
    .unwrap();
    let dev = TempDir::new().unwrap();
    fs::write(
        dev.path().join("package.json"),
        r#"{"devDependencies": {"@biomejs/biome": "^2"}}"#,
    )
    .unwrap();
    let direct = TempDir::new().unwrap();
    fs::write(
        direct.path().join("package.json"),
        r#"{"dependencies": {"markdownlint-cli2": "^0.24"}}"#,
    )
    .unwrap();

    assert_eq!(tools(&gaps(key.path(), DIR, &[])), ["prettier"]);
    assert_eq!(tools(&gaps(dev.path(), DIR, &[])), ["biome"]);
    assert_eq!(tools(&gaps(direct.path(), DIR, &[])), ["markdownlint"]);
}

#[test]
fn a_tool_in_package_json_only_is_told_to_create_its_file() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies": {"eslint": "^10"}}"#,
    )
    .unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [to_create(
            "eslint",
            "eslint.config.js",
            r#"{ ignores: [".references/**"] }"#
        )]
    );
}

#[test]
fn only_prettierignore_covers_prettier() {
    // Prettier ignores what its config or `package.json` says only through `.prettierignore`.
    let in_package = TempDir::new().unwrap();
    fs::write(
        in_package.path().join("package.json"),
        r#"{"prettier": {"ignore": [".references"]}}"#,
    )
    .unwrap();
    let in_rc = TempDir::new().unwrap();
    fs::write(in_rc.path().join(".prettierrc"), "# .references\n").unwrap();

    assert_eq!(
        gaps(in_package.path(), DIR, &[]),
        [to_create("prettier", ".prettierignore", ".references/")]
    );
    assert_eq!(
        gaps(in_rc.path(), DIR, &[]),
        [to_create("prettier", ".prettierignore", ".references/")]
    );
}

#[test]
fn a_package_json_that_does_not_parse_or_lacks_the_tool_says_nothing() {
    let broken = TempDir::new().unwrap();
    fs::write(broken.path().join("package.json"), "{").unwrap();
    let other = TempDir::new().unwrap();
    fs::write(
        other.path().join("package.json"),
        r#"{"devDependencies": {"left-pad": "1"}}"#,
    )
    .unwrap();

    assert_eq!(gaps(broken.path(), DIR, &[]), []);
    assert_eq!(gaps(other.path(), DIR, &[]), []);
}

#[test]
fn a_pre_commit_hook_puts_a_tool_in_use() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".pre-commit-config.yaml"),
        "repos:\n  - repo: https://github.com/psf/black\n    rev: 26.1.0\n    hooks:\n      - id: black\n",
    )
    .unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [to_create("black", "pyproject.toml", BLACK_FULL)]
    );
}

#[test]
fn a_tool_in_use_by_a_signal_is_covered_by_the_file_it_would_edit() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies": {"prettier": "3"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join(".prettierignore"), ".references/\n").unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn the_nearest_directory_with_a_signal_of_the_tool_is_the_one_read() {
    let (_repo, top, project) = worktree();
    fs::write(top.join(".prettierrc"), "{}").unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"devDependencies": {"prettier": "3"}}"#,
    )
    .unwrap();

    assert_eq!(
        gaps(&project, DIR, &[]),
        [to_create("prettier", ".prettierignore", ".references/")]
    );
}

#[test]
fn a_tool_named_in_the_ignore_list_is_left_out_whatever_shows_it() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies": {"prettier": "3"}}"#,
    )
    .unwrap();

    assert_eq!(gaps(dir.path(), DIR, &["prettier"]), []);
}
