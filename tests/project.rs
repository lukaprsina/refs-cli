use std::fs;

mod common;

use refs_cli::project::{find_root, init_root};
use tempfile::TempDir;

#[test]
fn the_root_is_the_nearest_directory_up_that_holds_a_refs_toml() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("refs.toml"), "").unwrap();
    let deep = dir.path().join("a/b");
    fs::create_dir_all(&deep).unwrap();

    let root = find_root(&deep).unwrap();

    assert_eq!(root, dunce::canonicalize(dir.path()).unwrap());
}

#[test]
fn no_refs_toml_anywhere_up_is_an_error() {
    let dir = TempDir::new().unwrap();

    let error = find_root(dir.path()).unwrap_err();

    assert_eq!(
        miette::Diagnostic::code(&error).unwrap().to_string(),
        "refs::project::no_config"
    );
}

mod outputs {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use miette::Diagnostic;
    use refs_cli::config::parse;
    use refs_cli::project::check_outputs;
    use tempfile::TempDir;

    fn codes(root: &TempDir, config: &str) -> Vec<String> {
        let config = parse(config).unwrap();
        check_outputs(root.path(), &config)
            .iter()
            .map(|e| e.code().unwrap().to_string())
            .collect()
    }

    #[test]
    #[cfg(unix)]
    fn a_references_dir_symlink_that_leaves_the_project_is_rejected() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        symlink(outside.path(), root.path().join(".references")).unwrap();

        assert_eq!(codes(&root, ""), ["refs::project::escapes_root"]);
    }

    #[test]
    #[cfg(unix)]
    fn a_missing_references_dir_under_a_symlinked_ancestor_that_leaves_is_rejected() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        symlink(outside.path(), root.path().join("out")).unwrap();
        let config = "[settings]\nreferences_dir = \"out/refs\"\n";

        assert_eq!(codes(&root, config), ["refs::project::escapes_root"]);
    }

    #[test]
    #[cfg(unix)]
    fn a_broken_symlink_is_rejected() {
        let root = TempDir::new().unwrap();
        symlink(root.path().join("nowhere"), root.path().join(".references")).unwrap();

        assert_eq!(codes(&root, ""), ["refs::project::broken_symlink"]);
    }

    #[test]
    fn a_references_dir_that_is_a_file_is_rejected() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".references"), "").unwrap();

        assert_eq!(codes(&root, ""), ["refs::project::not_a_directory"]);
    }

    #[test]
    fn an_agents_file_that_is_a_directory_is_rejected() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("AGENTS.md")).unwrap();

        assert_eq!(codes(&root, ""), ["refs::project::not_a_file"]);
    }

    #[test]
    #[cfg(unix)]
    fn a_project_with_nothing_yet_is_fine_and_a_symlinked_agents_file_inside_is_followed() {
        let root = TempDir::new().unwrap();
        assert!(codes(&root, "").is_empty());

        fs::write(root.path().join("real.md"), "").unwrap();
        symlink(root.path().join("real.md"), root.path().join("AGENTS.md")).unwrap();
        assert!(codes(&root, "").is_empty());
    }
}

mod observed {
    use std::fs;

    use crate::common::git;
    use refs_cli::config::parse;
    use refs_cli::plan::Exclude;
    use refs_cli::project::{ensure_exclude, observe};
    use tempfile::TempDir;

    const CONFIG: &str = r#"
[settings]
references_dir = "refs"
agents_files = ["AGENTS.md", "CLAUDE.md"]
"#;

    fn git_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q"]);
        dir
    }

    #[test]
    fn the_agent_files_are_read_and_a_missing_one_has_no_text() {
        let dir = git_project();
        fs::write(dir.path().join("AGENTS.md"), "# Notes\n").unwrap();

        let observed = observe(dir.path(), &parse(CONFIG).unwrap()).unwrap();

        assert_eq!(observed.references_dir, "refs");
        let files: Vec<_> = observed
            .agent_files
            .iter()
            .map(|f| (f.path.as_str(), f.text.as_deref()))
            .collect();
        assert_eq!(
            files,
            [("AGENTS.md", Some("# Notes\n")), ("CLAUDE.md", None)]
        );
    }

    #[test]
    fn an_unreadable_agent_file_is_an_error() {
        let dir = git_project();
        fs::create_dir(dir.path().join("AGENTS.md")).unwrap();

        let error = observe(dir.path(), &parse(CONFIG).unwrap()).unwrap_err();

        assert_eq!(
            miette::Diagnostic::code(&*error).unwrap().to_string(),
            "refs::block::read_failed"
        );
    }

    #[test]
    fn the_exclude_rule_is_missing_then_present_once_ensured() {
        let dir = git_project();
        let config = parse(CONFIG).unwrap();
        let exclude = |dir: &TempDir| observe(dir.path(), &config).unwrap().exclude;

        assert_eq!(exclude(&dir), Exclude::Missing);
        ensure_exclude(dir.path(), "refs").unwrap();
        assert_eq!(exclude(&dir), Exclude::Present);
        assert_eq!(rule_count(&dir.path().join(".git"), "/refs/"), 1);
    }

    #[test]
    fn the_rule_is_appended_on_its_own_line_to_a_file_without_a_final_newline() {
        let dir = git_project();
        fs::write(dir.path().join(".git/info/exclude"), "*.log").unwrap();

        ensure_exclude(dir.path(), "refs").unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap(),
            "*.log\n/refs/\n"
        );
    }

    #[test]
    fn a_rule_for_another_directory_does_not_count() {
        let dir = git_project();
        fs::write(dir.path().join(".git/info/exclude"), "/other/\n").unwrap();

        let observed = observe(dir.path(), &parse(CONFIG).unwrap()).unwrap();

        assert_eq!(observed.exclude, Exclude::Missing);
    }

    /// How many lines of `<git_dir>/info/exclude` are exactly `rule`.
    fn rule_count(git_dir: &std::path::Path, rule: &str) -> usize {
        fs::read_to_string(git_dir.join("info/exclude"))
            .unwrap()
            .lines()
            .filter(|l| *l == rule)
            .count()
    }

    /// A repository with one commit, so that it can have linked worktrees.
    fn repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["commit", "-q", "--allow-empty", "-m", "x"]);
        dir
    }

    #[test]
    fn a_linked_worktree_gets_the_rule_in_the_common_git_dir() {
        let main = repo();
        let linked = TempDir::new().unwrap();
        let linked_path = linked.path().join("wt");
        git(
            main.path(),
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                linked_path.to_str().unwrap(),
            ],
        );
        let config = parse(CONFIG).unwrap();
        let exclude = || observe(&linked_path, &config).unwrap().exclude;

        assert_eq!(exclude(), Exclude::Missing);
        ensure_exclude(&linked_path, "refs").unwrap();

        assert_eq!(exclude(), Exclude::Present);
        assert_eq!(rule_count(&main.path().join(".git"), "/refs/"), 1);
    }

    #[test]
    fn a_project_below_the_repository_top_gets_a_rule_anchored_to_the_top() {
        let top = repo();
        let project = top.path().join("packages/app");
        fs::create_dir_all(&project).unwrap();
        let config = parse(CONFIG).unwrap();
        let exclude = || observe(&project, &config).unwrap().exclude;

        assert_eq!(exclude(), Exclude::Missing);
        ensure_exclude(&project, "refs").unwrap();

        assert_eq!(exclude(), Exclude::Present);
        let git_dir = top.path().join(".git");
        assert_eq!(rule_count(&git_dir, "/packages/app/refs/"), 1);
        assert_eq!(rule_count(&git_dir, "/refs/"), 0);
    }

    #[test]
    fn a_bare_repository_has_nowhere_for_the_rule() {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q", "--bare"]);

        let observed = observe(dir.path(), &parse(CONFIG).unwrap()).unwrap();

        assert_eq!(observed.exclude, Exclude::NoGit);
        ensure_exclude(dir.path(), "refs").unwrap();
        assert_eq!(rule_count(dir.path(), "/refs/"), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_with_a_newline_gets_no_rule_rather_than_a_rule_in_the_wrong_place() {
        let parent = TempDir::new().unwrap();
        let dir = parent.path().join("a\nb");
        fs::create_dir(&dir).unwrap();
        git(&dir, &["init", "-q"]);

        let observed = observe(&dir, &parse(CONFIG).unwrap()).unwrap();

        assert_eq!(observed.exclude, Exclude::NoGit);
        ensure_exclude(&dir, "refs").unwrap();
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_directory_without_a_git_directory_has_nowhere_for_the_rule() {
        let dir = TempDir::new().unwrap();

        let observed = observe(dir.path(), &parse(CONFIG).unwrap()).unwrap();

        assert_eq!(observed.exclude, Exclude::NoGit);
    }
}

#[test]
fn the_config_is_written_atomically_and_read_back() {
    use refs_cli::project::{read_config, write_config};

    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("refs.toml"), "old\n").unwrap();

    write_config(dir.path(), "[settings]\n").unwrap();

    assert_eq!(read_config(dir.path()).unwrap(), "[settings]\n");
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["refs.toml"], "no temp file is left behind");
}

mod init_root_rule {
    use super::*;

    fn canonical(dir: &TempDir) -> std::path::PathBuf {
        dunce::canonicalize(dir.path()).unwrap()
    }

    #[test]
    fn with_no_project_it_is_the_git_worktree_root_or_else_the_start() {
        let dir = TempDir::new().unwrap();
        let deep = dir.path().join("a/b");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            init_root(&deep, false).unwrap(),
            dunce::canonicalize(&deep).unwrap()
        );

        crate::common::git(dir.path(), &["init", "-q"]);
        assert_eq!(init_root(&deep, false).unwrap(), canonical(&dir));
    }

    #[test]
    fn an_existing_project_is_its_own_root_but_is_not_entered_from_below() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("refs.toml"), "").unwrap();
        let deep = dir.path().join("a");
        fs::create_dir_all(&deep).unwrap();

        assert_eq!(init_root(dir.path(), false).unwrap(), canonical(&dir));
        let error = init_root(&deep, false).unwrap_err();
        assert_eq!(
            miette::Diagnostic::code(&error).unwrap().to_string(),
            "refs::project::nested"
        );
    }

    #[test]
    fn here_always_means_the_start() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("refs.toml"), "").unwrap();
        let deep = dir.path().join("a");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            init_root(&deep, true).unwrap(),
            dunce::canonicalize(&deep).unwrap()
        );
    }
}
