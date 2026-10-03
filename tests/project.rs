use std::fs;

use refs_cli::project::find_root;
use tempfile::TempDir;

#[test]
fn the_root_is_the_nearest_directory_up_that_holds_a_refs_toml() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("refs.toml"), "").unwrap();
    let deep = dir.path().join("a/b");
    fs::create_dir_all(&deep).unwrap();

    let root = find_root(&deep).unwrap();

    assert_eq!(root, dir.path().canonicalize().unwrap());
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
    fn a_references_dir_symlink_that_leaves_the_project_is_rejected() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        symlink(outside.path(), root.path().join(".references")).unwrap();

        assert_eq!(codes(&root, ""), ["refs::project::escapes_root"]);
    }

    #[test]
    fn a_missing_references_dir_under_a_symlinked_ancestor_that_leaves_is_rejected() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        symlink(outside.path(), root.path().join("out")).unwrap();
        let config = "[settings]\nreferences_dir = \"out/refs\"\n";

        assert_eq!(codes(&root, config), ["refs::project::escapes_root"]);
    }

    #[test]
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
        fs::create_dir_all(dir.path().join(".git/info")).unwrap();
        dir
    }

    #[test]
    fn the_agent_files_are_read_and_a_missing_one_has_no_text() {
        let dir = git_project();
        fs::write(dir.path().join("AGENTS.md"), "# Notes\n").unwrap();

        let observed = observe(dir.path(), &parse(CONFIG).unwrap(), vec!["a".into()]).unwrap();

        assert_eq!(observed.references_dir, "refs");
        assert_eq!(observed.listing, ["a"]);
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

        let error = observe(dir.path(), &parse(CONFIG).unwrap(), vec![]).unwrap_err();

        assert_eq!(
            miette::Diagnostic::code(&*error).unwrap().to_string(),
            "refs::block::read_failed"
        );
    }

    #[test]
    fn the_exclude_rule_is_missing_then_present_once_ensured() {
        let dir = git_project();
        let config = parse(CONFIG).unwrap();
        let exclude = |dir: &TempDir| observe(dir.path(), &config, vec![]).unwrap().exclude;

        assert_eq!(exclude(&dir), Exclude::Missing);
        ensure_exclude(dir.path(), "refs").unwrap();
        assert_eq!(exclude(&dir), Exclude::Present);
        assert_eq!(
            fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap(),
            "/refs/\n"
        );
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

        let observed = observe(dir.path(), &parse(CONFIG).unwrap(), vec![]).unwrap();

        assert_eq!(observed.exclude, Exclude::Missing);
    }

    #[test]
    fn a_directory_without_a_git_directory_has_nowhere_for_the_rule() {
        let dir = TempDir::new().unwrap();

        let observed = observe(dir.path(), &parse(CONFIG).unwrap(), vec![]).unwrap();

        assert_eq!(observed.exclude, Exclude::NoGit);
    }
}
