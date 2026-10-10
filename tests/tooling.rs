//! Which tools leave the references directory in reach, over real directories.

use std::fs;
use std::path::{Path, PathBuf};

mod common;

use refs_cli::tooling::{Gap, Tool, gaps};
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

fn gap(tool: Tool, file: &str, snippet: &str) -> Gap {
    Gap {
        tool,
        file: file.into(),
        snippet: snippet.into(),
    }
}

fn tools(found: &[Gap]) -> Vec<Tool> {
    found.iter().map(|gap| gap.tool).collect()
}

#[test]
fn an_eslint_flat_config_that_does_not_name_the_directory_is_a_gap() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("eslint.config.js"), "export default [];\n").unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [gap(
            Tool::Eslint,
            "eslint.config.js",
            r#"{ ignores: [".references/**"] }"#
        )]
    );
}

#[test]
fn a_config_that_names_the_directory_is_covered() {
    let dir = TempDir::new().unwrap();
    let config = r#"export default [{ ignores: [".references/**"] }];"#;
    fs::write(dir.path().join("eslint.config.js"), config).unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn a_project_with_no_tool_config_has_no_gap() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("README.md"), "hi").unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn legacy_eslint_is_fixed_in_eslintignore() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".eslintrc.json"), "{}").unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [gap(Tool::Eslint, ".eslintignore", ".references/")]
    );
}

#[test]
fn legacy_eslint_is_covered_by_the_directory_in_eslintignore() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".eslintrc.json"), "{}").unwrap();
    fs::write(dir.path().join(".eslintignore"), "dist\n.references/\n").unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn a_flat_config_is_not_covered_by_eslintignore_which_eslint_then_ignores() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("eslint.config.js"), "export default [];").unwrap();
    fs::write(dir.path().join(".eslintignore"), ".references/\n").unwrap();

    assert_eq!(
        gaps(dir.path(), DIR, &[]),
        [gap(
            Tool::Eslint,
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
        [gap(Tool::Prettier, ".prettierignore", ".references/")]
    );
}

#[test]
fn a_prettierignore_alone_counts_as_using_prettier() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".prettierignore"), "dist\n").unwrap();

    assert_eq!(tools(&gaps(dir.path(), DIR, &[])), [Tool::Prettier]);
}

#[test]
fn prettier_is_covered_by_the_directory_in_prettierignore() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
    fs::write(dir.path().join(".prettierignore"), ".references\n").unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn oxlint_and_tsc_name_their_own_key() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".oxlintrc.json"), "{}").unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();

    let mut found = gaps(dir.path(), DIR, &[]);
    found.sort_by_key(|gap| gap.file.clone());

    assert_eq!(
        found,
        [
            gap(
                Tool::Oxlint,
                ".oxlintrc.json",
                r#""ignorePatterns": [".references/"]"#
            ),
            gap(Tool::Tsc, "tsconfig.json", r#""exclude": [".references"]"#),
        ]
    );
}

#[test]
fn tsconfig_variants_are_not_detected() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("tsconfig.build.json"), "{}").unwrap();

    assert_eq!(gaps(dir.path(), DIR, &[]), []);
}

#[test]
fn an_ignored_tool_is_not_reported() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("eslint.config.js"), "").unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();

    assert_eq!(tools(&gaps(dir.path(), DIR, &[Tool::Eslint])), [Tool::Tsc]);
}

#[test]
fn a_config_further_up_is_found_with_the_path_from_its_directory() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("eslint.config.js"), "").unwrap();

    assert_eq!(
        gaps(&project, DIR, &[]),
        [gap(
            Tool::Eslint,
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
    fs::write(top.join("tsconfig.json"), "{}").unwrap();
    fs::write(project.join("eslint.config.js"), "").unwrap();

    let mut found = tools(&gaps(&project, DIR, &[]));
    found.sort_by_key(|tool| *tool as u8);
    assert_eq!(found, [Tool::Eslint, Tool::Tsc]);
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
