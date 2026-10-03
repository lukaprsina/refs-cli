mod common;

use std::fs;

use refs_cli::init::init;
use tempfile::TempDir;

#[test]
fn init_creates_the_config_and_an_agent_file_with_an_empty_block() {
    let dir = TempDir::new().unwrap();
    let done = init(dir.path()).unwrap();

    assert!(done.config_created);
    let config = fs::read_to_string(dir.path().join("refs.toml")).unwrap();
    let parsed = refs_cli::config::parse(&config).unwrap();
    assert!(parsed.repos.is_empty() && parsed.groups.is_empty());
    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(agents.starts_with("<!-- BEGIN:refs -->\n"), "{agents}");
    assert!(agents.ends_with("<!-- END:refs -->\n"), "{agents}");
    assert!(!agents.contains("###"), "no group headings: {agents}");
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
fn init_keeps_an_existing_config_and_the_rest_of_an_existing_agent_file() {
    let dir = TempDir::new().unwrap();
    let config = "[settings]\nagents_files = [\"AGENTS.md\", \"docs/AGENT.md\"]\n# mine\n";
    fs::write(dir.path().join("refs.toml"), config).unwrap();
    fs::write(dir.path().join("AGENTS.md"), "# Rules\n\nBe kind.\n").unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();

    init(dir.path()).unwrap();

    assert_eq!(
        fs::read_to_string(dir.path().join("refs.toml")).unwrap(),
        config
    );
    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(
        agents.starts_with("# Rules\n\nBe kind.\n\n<!-- BEGIN:refs -->"),
        "{agents}"
    );
    let second = fs::read_to_string(dir.path().join("docs/AGENT.md")).unwrap();
    assert!(second.starts_with("<!-- BEGIN:refs -->"), "{second}");
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

#[test]
fn init_refuses_an_agent_file_with_broken_markers_and_writes_no_block() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("AGENTS.md"), "<!-- BEGIN:refs -->\n").unwrap();
    let err = init(dir.path()).unwrap_err();
    assert!(matches!(err, refs_cli::diagnostic::InitError::Block(_)));
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        "<!-- BEGIN:refs -->\n"
    );
}

#[test]
fn a_second_init_does_not_rewrite_the_agent_file() {
    let dir = TempDir::new().unwrap();
    init(dir.path()).unwrap();
    let path = dir.path().join("AGENTS.md");
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    init(dir.path()).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}
