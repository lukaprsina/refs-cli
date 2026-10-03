//! `sync::sync_edited`: an edit of `refs.toml` is written only once stage 1 (the lock) has
//! passed against it, and then the project is synced to it.

use std::fs;

use refs_cli::config::parse;
use refs_cli::source::fake::{Call, FakeSource, Method};
use refs_cli::sync::{Outcome, SyncFlags, sync_edited};
use tempfile::TempDir;

const BEFORE: &str = "[repos.a]\nurl = \"https://github.com/o/a\"\n";
const AFTER: &str =
    "[repos.a]\nurl = \"https://github.com/o/a\"\n\n[repos.b]\nurl = \"https://github.com/o/b\"\n";

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    let git = std::process::Command::new("git")
        .current_dir(dir.path())
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(git.success());
    fs::write(dir.path().join("refs.toml"), BEFORE).unwrap();
    dir
}

fn read(dir: &TempDir, name: &str) -> Option<String> {
    fs::read_to_string(dir.path().join(name)).ok()
}

#[test]
fn a_passing_edit_is_written_with_its_lock_and_synced() {
    let dir = project();
    let source = FakeSource::new();

    let (written, report) = sync_edited(&source, dir.path(), AFTER, &SyncFlags::default());

    assert!(written);
    assert_eq!(report.outcome, Outcome::InSync);
    assert_eq!(read(&dir, "refs.toml").unwrap(), AFTER);
    assert!(read(&dir, "refs.lock").unwrap().contains("id = \"b\""));
    assert!(source.calls().contains(&Call::Materialise {
        id: "b".into(),
        offline: false
    }));
    assert!(read(&dir, "AGENTS.md").unwrap().contains("[b @"));
}

#[test]
fn a_stage_one_failure_writes_neither_refs_toml_nor_the_lock() {
    let dir = project();
    let source = FakeSource::new();
    source.fail("b", Method::Resolve, "no such ref");

    let (written, report) = sync_edited(&source, dir.path(), AFTER, &SyncFlags::default());

    assert!(!written);
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(read(&dir, "refs.toml").unwrap(), BEFORE);
    assert_eq!(read(&dir, "refs.lock"), None);
    assert!(
        !source
            .calls()
            .iter()
            .any(|c| matches!(c, Call::Materialise { .. }))
    );
}

#[test]
fn a_stage_two_failure_keeps_the_edit_and_the_lock() {
    let dir = project();
    let source = FakeSource::new();
    source.fail("b", Method::Materialise, "network down");

    let (written, report) = sync_edited(&source, dir.path(), AFTER, &SyncFlags::default());

    assert!(written);
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(read(&dir, "refs.toml").unwrap(), AFTER);
    assert!(read(&dir, "refs.lock").unwrap().contains("id = \"b\""));
    parse(&read(&dir, "refs.toml").unwrap()).unwrap();
}
