//! `sync::edit`: an edit of `refs.toml` is written only once stage 1 (the lock) has passed
//! against it, or the Lock could be pruned, and then the project is synced to it.

use std::fs;

use refs_cli::config::parse;
use refs_cli::diagnostic::SourceError;
use refs_cli::edit::{AddRepo, Edit, Target};
use refs_cli::source::fake::{Call, FakeSource, Method};
use refs_cli::sync::{Change, Edited, Hint, Outcome, SyncFlags, edit};
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
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
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

    assert_eq!(edited.change, Change::Blocked);
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

    assert_eq!(edited.change, Change::Blocked);
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

    assert_eq!(edited.change, Change::Blocked);
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

fn verifies(source: &FakeSource) -> usize {
    source
        .calls()
        .iter()
        .filter(|c| matches!(c, Call::Verify { .. }))
        .count()
}

#[test]
fn a_shrinking_edit_prunes_the_lock_and_is_not_blocked_by_a_broken_repo() {
    for the_edit in [Edit::Remove("a"), Edit::Disable(Target::Repo("a"))] {
        let (dir, source) = synced_then_b_breaks();
        let verified = verifies(&source);

        let edited = run_edit(&dir, &source, &the_edit);

        assert_eq!(edited.change, Change::Written, "{the_edit:?}");
        assert_eq!(edited.report.outcome, Outcome::InSync, "{the_edit:?}");
        let lock = read(&dir, "refs.lock").unwrap();
        assert!(
            lock.contains("id = \"b\"")
                && lock.contains("id = \"c\"")
                && !lock.contains("id = \"a\""),
            "{lock}"
        );
        assert!(!checked_out(&source, "a"), "the removed Checkout goes");
        assert!(checked_out(&source, "b") && checked_out(&source, "c"));
        let block = read(&dir, "AGENTS.md").unwrap();
        assert!(
            block.contains("[b @") && block.contains("[c @") && !block.contains("[a @"),
            "{block}"
        );
        assert_eq!(verifies(&source), verified, "nothing is verified");
    }
}

#[test]
fn an_edit_that_cannot_prune_is_blocked_by_a_broken_repo_and_hints_at_no_sync() {
    // Nothing is locked yet, so there is no Lock to prune.
    let (dir, source) = with_broken_b();
    let config = read(&dir, "refs.toml");

    let edited = run_edit(&dir, &source, &Edit::Disable(Target::Repo("a")));

    assert_eq!(edited.change, Change::Blocked);
    assert_eq!(edited.report.outcome, Outcome::Failed);
    assert_eq!(edited.report.hint, Some(Hint::ConfigUnchangedTryNoSync));
    assert_eq!(read(&dir, "refs.toml"), config);
    assert_eq!(read(&dir, "refs.lock"), None);
    assert!(messages(&edited).contains("`b`"), "{}", messages(&edited));
}

fn no_sync_add(dir: &TempDir, source: &FakeSource, name: &str) -> Edited {
    let req = AddRepo {
        url: format!("https://github.com/o/{name}"),
        ..AddRepo::default()
    };
    edit(
        dir.path(),
        &Edit::Add(&req),
        true,
        || Ok::<_, SourceError>(source),
        &SyncFlags::default(),
    )
}

#[test]
fn an_edit_carries_the_hint_for_what_it_left_behind() {
    // Written without a sync: the project is incomplete.
    let (dir, source) = (project(), FakeSource::new());
    let edited = no_sync_add(&dir, &source, "b");
    assert_eq!(edited.change, Change::Written);
    assert_eq!(edited.report.hint, Some(Hint::RunSync));

    // Nothing to write and nothing to sync.
    let edited = edit(
        dir.path(),
        &Edit::Enable(Target::Repo("a")),
        true,
        || Ok::<_, SourceError>(&source),
        &SyncFlags::default(),
    );
    assert_eq!(edited.change, Change::Unchanged);
    assert_eq!(edited.report.hint, None);

    // Written and synced.
    let (dir, source) = (project(), FakeSource::new());
    let edited = add(&dir, &source, "b", &SyncFlags::default());
    assert_eq!(edited.report.hint, None);

    // Written, then the checkout failed.
    let (dir, source) = (project(), FakeSource::new());
    source.fail("b", Method::Materialise, "disk full");
    let edited = add(&dir, &source, "b", &SyncFlags::default());
    assert_eq!(edited.change, Change::Written);
    assert_eq!(edited.report.outcome, Outcome::Failed);
    assert_eq!(edited.report.hint, Some(Hint::FixOrRemoveThenSync));

    // Blocked by stage 1: `refs.toml` is as it was, and `--no-sync` would write it.
    let (dir, source) = (project(), FakeSource::new());
    source.fail("b", Method::Resolve, "no such ref");
    let edited = add(&dir, &source, "b", &SyncFlags::default());
    assert_eq!(edited.change, Change::Blocked);
    assert_eq!(edited.report.hint, Some(Hint::ConfigUnchangedTryNoSync));

    // Rejected as an edit: `--no-sync` would not help.
    let (dir, source) = (project(), FakeSource::new());
    let edited = run_edit(&dir, &source, &Edit::Remove("nope"));
    assert_eq!(edited.change, Change::Rejected);
    assert_eq!(edited.report.hint, Some(Hint::ConfigUnchanged));

    // Nothing to write, and the sync it fell back to failed.
    let (dir, source) = (project(), FakeSource::new());
    source.fail("a", Method::Resolve, "no such ref");
    let edited = enable(&dir, &source, "a");
    assert_eq!(edited.change, Change::Unchanged);
    assert_eq!(edited.report.hint, Some(Hint::RunSyncOnceFixed));
}
