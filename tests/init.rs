mod common;

use std::fs;

use refs_cli::init::init;
use tempfile::TempDir;

#[test]
fn init_creates_the_config_and_no_block_while_no_repo_is_active() {
    let dir = TempDir::new().unwrap();
    let done = init(dir.path()).unwrap();

    assert!(done.config_created);
    let config = fs::read_to_string(dir.path().join("refs.toml")).unwrap();
    let parsed = refs_cli::config::parse(&config).unwrap();
    assert!(parsed.repos.is_empty() && parsed.groups.is_empty());
    assert!(!dir.path().join("AGENTS.md").exists());
}

fn snapshot(dir: &TempDir) -> Vec<(String, String)> {
    let mut files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn a_second_init_changes_nothing() {
    let dir = TempDir::new().unwrap();
    init(dir.path()).unwrap();
    let first = snapshot(&dir);
    let done = init(dir.path()).unwrap();
    assert!(!done.config_created);
    assert_eq!(snapshot(&dir), first);
}

#[test]
fn init_keeps_an_existing_config_and_leaves_agent_files_alone() {
    let dir = TempDir::new().unwrap();
    let config = "[settings]\nagents_files = [\"AGENTS.md\"]\n# mine\n";
    fs::write(dir.path().join("refs.toml"), config).unwrap();
    fs::write(dir.path().join("AGENTS.md"), "# Rules\n\nBe kind.\n").unwrap();

    init(dir.path()).unwrap();

    assert_eq!(
        fs::read_to_string(dir.path().join("refs.toml")).unwrap(),
        config
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        "# Rules\n\nBe kind.\n"
    );
}

#[test]
fn init_excludes_the_references_dir_in_a_git_repo() {
    let dir = TempDir::new().unwrap();
    common::git(dir.path(), &["init", "-q"]);
    init(dir.path()).unwrap();
    init(dir.path()).unwrap();
    let exclude = fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap();
    assert_eq!(
        exclude.lines().filter(|l| *l == "/.references/").count(),
        1,
        "{exclude}"
    );
}
