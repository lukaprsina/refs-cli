mod common;

use std::fs;

use refs_cli::cli::run_with;
use refs_cli::source::fake::FakeSource;
use tempfile::TempDir;

/// `refs init` in `dir`: the exit code and everything it printed.
fn init(dir: &std::path::Path) -> (u8, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(
        ["refs", "init", "--no-color"].map(Into::into),
        dir,
        |_, _| Ok(Box::new(FakeSource::new())),
        &mut out,
        &mut err,
    );
    out.extend(err);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn init_creates_the_config_and_no_block_while_no_repo_is_active() {
    let dir = TempDir::new().unwrap();
    let (code, said) = init(dir.path());

    assert_eq!(code, 0);
    assert!(said.contains("created refs.toml"), "{said}");
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
    init(dir.path());
    let first = snapshot(&dir);
    let (code, said) = init(dir.path());
    assert_eq!(code, 0);
    assert!(said.contains("nothing changed"), "{said}");
    assert_eq!(snapshot(&dir), first);
}

#[test]
fn init_keeps_an_existing_config_and_leaves_agent_files_alone() {
    let dir = TempDir::new().unwrap();
    let config = "[settings]\nagents_files = [\"AGENTS.md\"]\n# mine\n";
    fs::write(dir.path().join("refs.toml"), config).unwrap();
    fs::write(dir.path().join("AGENTS.md"), "# Rules\n\nBe kind.\n").unwrap();

    init(dir.path());

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
    init(dir.path());
    init(dir.path());
    let exclude = fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap();
    assert_eq!(
        exclude.lines().filter(|l| *l == "/.references/").count(),
        1,
        "{exclude}"
    );
}
