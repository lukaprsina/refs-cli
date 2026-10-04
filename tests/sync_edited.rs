//! `sync::edit`: an edit of `refs.toml` that grows the active set is written only once stage 1
//! (the lock) has passed against it, and then the project is synced to it.

use std::fs;

use refs_cli::config::parse;
use refs_cli::diagnostic::SourceError;
use refs_cli::edit::{AddRepo, Edit, Target};
use refs_cli::source::fake::{Call, FakeSource, Method};
use refs_cli::sync::{Change, Edited, Outcome, SyncFlags, edit};
use tempfile::TempDir;

const BEFORE: &str = "[repos.a]\nurl = \"https://github.com/o/a\"\n";
const AFTER: &str =
    "[repos.a]\nurl = \"https://github.com/o/a\"\n\n[repos.b]\nurl = \"https://github.com/o/b\"\n";

/// `refs add https://github.com/o/<name>`, synced.
fn add(dir: &TempDir, source: &FakeSource, name: &str, flags: &SyncFlags) -> Edited {
    let req = AddRepo {
        url: format!("https://github.com/o/{name}"),
        ..AddRepo::default()
    };
    edit(
        dir.path(),
        &Edit::Add(&req),
        false,
        || Ok::<_, SourceError>(source),
        flags,
    )
}

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

    let edited = add(&dir, &source, "b", &SyncFlags::default());
    let report = &edited.report;

    assert_eq!(edited.change, Change::Written);
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

    let edited = add(&dir, &source, "b", &SyncFlags::default());
    let report = &edited.report;

    assert_eq!(edited.change, Change::Rejected);
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

    let edited = add(&dir, &source, "b", &SyncFlags::default());
    let report = &edited.report;

    assert_eq!(edited.change, Change::Written);
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(read(&dir, "refs.toml").unwrap(), AFTER);
    assert!(read(&dir, "refs.lock").unwrap().contains("id = \"b\""));
    parse(&read(&dir, "refs.toml").unwrap()).unwrap();
}

#[test]
fn an_edit_that_changes_nothing_writes_nothing_but_still_syncs() {
    let dir = project();
    let source = FakeSource::new();

    let edited = enable(&dir, &source, "a");

    assert_eq!(edited.change, Change::Unchanged);
    assert_eq!(edited.report.outcome, Outcome::InSync);
    assert!(read(&dir, "refs.lock").unwrap().contains("id = \"a\""));
}

#[test]
fn an_edit_verifies_every_repo_once() {
    let dir = project();
    let source = FakeSource::new();

    add(&dir, &source, "b", &SyncFlags::default());

    let verified: Vec<_> = source
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Verify { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(
        verified.len(),
        parse(AFTER).unwrap().repos.len(),
        "{verified:?}"
    );
}

#[test]
fn an_edit_reports_how_the_lock_differed_from_it() {
    let dir = project();
    let source = FakeSource::new();
    enable(&dir, &source, "a");

    let edited = add(&dir, &source, "b", &SyncFlags::default());

    assert_eq!(edited.report.drift.len(), 1, "{:?}", edited.report.drift);
}

#[test]
fn an_edit_that_cannot_resolve_offline_is_rejected_and_writes_nothing() {
    let dir = project();
    let source = FakeSource::new();
    let flags = SyncFlags {
        offline: true,
        ..SyncFlags::default()
    };

    let edited = add(&dir, &source, "b", &flags);

    assert_eq!(edited.change, Change::Rejected);
    assert_eq!(read(&dir, "refs.toml").unwrap(), BEFORE);
    assert_eq!(read(&dir, "refs.lock"), None);
    assert!(source.calls().is_empty(), "{:?}", source.calls());
}

/// `refs enable <id>`, synced.
fn enable(dir: &TempDir, source: &FakeSource, id: &str) -> Edited {
    edit(
        dir.path(),
        &Edit::Enable(Target::Repo(id)),
        false,
        || Ok::<_, SourceError>(source),
        &SyncFlags::default(),
    )
}

const ABC: &str = "[repos.a]\nurl = \"https://github.com/o/a\"\n\n[repos.b]\nurl = \"https://github.com/o/b\"\n\n[repos.c]\nurl = \"https://github.com/o/c\"\n";

/// `a`, `b` (which does not resolve) and `c`, none of them locked yet.
fn with_broken_b() -> (TempDir, FakeSource) {
    let dir = project();
    fs::write(dir.path().join("refs.toml"), ABC).unwrap();
    let source = FakeSource::new();
    source.fail("b", Method::Resolve, "no such ref");
    (dir, source)
}

fn run_edit(dir: &TempDir, source: &FakeSource, the_edit: &Edit) -> Edited {
    edit(
        dir.path(),
        the_edit,
        false,
        || Ok::<_, SourceError>(source),
        &SyncFlags::default(),
    )
}

fn messages(edited: &Edited) -> String {
    edited
        .report
        .diagnostics
        .iter()
        .map(|d| format!("{d:?}"))
        .collect()
}

#[test]
fn removing_the_broken_repo_succeeds() {
    let (dir, source) = with_broken_b();

    let edited = run_edit(&dir, &source, &Edit::Remove("b"));

    assert_eq!(edited.change, Change::Written);
    assert_eq!(edited.report.outcome, Outcome::InSync);
    assert!(!read(&dir, "refs.toml").unwrap().contains("[repos.b]"));
}

#[test]
fn disabling_a_repo_writes_the_edit_and_the_lock_entries_it_can_then_reports_the_broken_one() {
    let (dir, source) = with_broken_b();

    let edited = run_edit(&dir, &source, &Edit::Disable(Target::Repo("a")));

    assert_eq!(edited.change, Change::Written);
    assert_eq!(edited.report.outcome, Outcome::Failed);
    assert!(read(&dir, "refs.toml").unwrap().contains("enabled = false"));
    let lock = read(&dir, "refs.lock").unwrap();
    assert!(
        lock.contains("id = \"c\"") && !lock.contains("id = \"a\""),
        "{lock}"
    );
    assert!(messages(&edited).contains("`b`"), "{}", messages(&edited));
}

#[test]
fn removing_a_repo_writes_the_edit_though_another_repo_fails() {
    let (dir, source) = with_broken_b();

    let edited = run_edit(&dir, &source, &Edit::Remove("a"));

    assert_eq!(edited.change, Change::Written);
    assert_eq!(edited.report.outcome, Outcome::Failed);
    assert!(!read(&dir, "refs.toml").unwrap().contains("[repos.a]"));
}

#[test]
fn enabling_a_repo_that_does_not_resolve_writes_nothing_and_names_it() {
    let dir = project();
    fs::write(
        dir.path().join("refs.toml"),
        format!("{BEFORE}\n[repos.b]\nurl = \"https://github.com/o/b\"\nenabled = false\n"),
    )
    .unwrap();
    let source = FakeSource::new();
    source.fail("b", Method::Resolve, "no such ref");
    let config = read(&dir, "refs.toml");

    let edited = run_edit(&dir, &source, &Edit::Enable(Target::Repo("b")));

    assert_eq!(edited.change, Change::Rejected);
    assert_eq!(read(&dir, "refs.toml"), config);
    assert_eq!(read(&dir, "refs.lock"), None);
    assert!(messages(&edited).contains("`b`"), "{}", messages(&edited));
}

/// `a`, `b` and `c` synced, then `b` stops verifying (its remote broke).
fn synced_then_b_breaks() -> (TempDir, FakeSource) {
    let dir = project();
    fs::write(dir.path().join("refs.toml"), ABC).unwrap();
    let source = FakeSource::new();
    let first = run_edit(&dir, &source, &Edit::Enable(Target::Repo("a")));
    assert_eq!(first.report.outcome, Outcome::InSync);
    source.fail("b", Method::Verify, "remote gone");
    (dir, source)
}

fn checked_out(source: &FakeSource, id: &str) -> bool {
    !matches!(
        refs_cli::source::Source::inspect(source, id).unwrap(),
        refs_cli::source::Observed::Absent
    )
}

#[test]
fn a_disable_with_a_broken_repo_still_syncs_the_checkouts_and_the_block_of_the_others() {
    let (dir, source) = synced_then_b_breaks();
    let block = read(&dir, "AGENTS.md").unwrap();
    assert!(block.contains("[a @"), "{block}");

    let edited = run_edit(&dir, &source, &Edit::Disable(Target::Repo("a")));

    assert_eq!(edited.change, Change::Written);
    assert_eq!(edited.report.outcome, Outcome::Failed);
    assert!(messages(&edited).contains("`b`"), "{}", messages(&edited));
    assert!(!checked_out(&source, "a"), "the disabled Checkout goes");
    assert!(
        checked_out(&source, "b"),
        "the broken Repo's Checkout is left alone"
    );
    assert!(checked_out(&source, "c"));
    let after = read(&dir, "AGENTS.md").unwrap();
    assert!(
        after.contains("[c @") && !after.contains("[a @") && !after.contains("[b @"),
        "{after}"
    );
}

#[test]
fn a_remove_with_a_broken_repo_drops_the_removed_checkout_too() {
    let (dir, source) = synced_then_b_breaks();

    let edited = run_edit(&dir, &source, &Edit::Remove("a"));

    assert_eq!(edited.report.outcome, Outcome::Failed);
    assert!(!checked_out(&source, "a"));
    assert!(checked_out(&source, "b"));
}
