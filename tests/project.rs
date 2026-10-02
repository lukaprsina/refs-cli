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
