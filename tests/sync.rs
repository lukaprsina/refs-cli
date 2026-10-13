use std::fs;

use refs_cli::config::{Config, parse};
use refs_cli::lock::Lock;
use refs_cli::plan::{Drift, LockFlags};
use refs_cli::source::fake::{Call, FakeSource, Method};
use refs_cli::source::{Observed, Source};
use refs_cli::sync::{Outcome, Report, SyncFlags, lock, sync};
use tempfile::TempDir;

const A: &str = r#"
[repos.a]
url = "https://github.com/o/a"
ref = "next"
paths = ["docs"]
"#;

/// A project directory that is a git repository, with the fake as its checkouts.
struct Project {
    dir: TempDir,
    source: FakeSource,
    config: Config,
}

impl Project {
    fn new(config: &str) -> Project {
        let dir = TempDir::new().unwrap();
        let status = std::process::Command::new("git")
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_WORK_TREE")
            .current_dir(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap();
        assert!(status.success());
        Project {
            dir,
            source: FakeSource::new(),
            config: parse(config).unwrap(),
        }
    }

    fn sync(&self, flags: &SyncFlags) -> Report {
        sync(&self.source, self.dir.path(), &self.config, flags)
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join(name)
    }

    fn read(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.path(name)).ok()
    }

    fn lock(&self) -> Option<Lock> {
        self.read("refs.lock").map(|t| Lock::parse(&t).unwrap())
    }
}

fn codes(report: &Report) -> Vec<String> {
    report
        .diagnostics
        .iter()
        .filter_map(|d| d.code().map(|c| c.to_string()))
        .collect()
}

const AB: &str = r#"
[repos.a]
url = "https://github.com/o/a"
[repos.b]
url = "https://github.com/o/b"
"#;

fn check() -> SyncFlags {
    SyncFlags {
        check: true,
        ..SyncFlags::default()
    }
}

fn mtime(p: &Project, name: &str) -> std::time::SystemTime {
    fs::metadata(p.path(name)).unwrap().modified().unwrap()
}

fn synced(config: &str) -> Project {
    let p = Project::new(config);
    assert_eq!(p.sync(&SyncFlags::default()).outcome, Outcome::InSync);
    p
}

#[test]
fn a_second_sync_resolves_and_writes_nothing_but_still_verifies() {
    let p = synced(AB);
    let before = (mtime(&p, "refs.lock"), mtime(&p, "AGENTS.md"));
    let seen = p.source.calls().len();
    std::thread::sleep(std::time::Duration::from_millis(20));

    let report = p.sync(&SyncFlags::default());

    assert_eq!(report.outcome, Outcome::InSync);
    assert_eq!(before, (mtime(&p, "refs.lock"), mtime(&p, "AGENTS.md")));
    assert_eq!(
        p.source.calls()[seen..],
        [
            Call::Verify {
                id: "a".into(),
                offline: false
            },
            Call::Verify {
                id: "b".into(),
                offline: false
            }
        ]
    );
}

#[test]
fn one_failed_repo_does_not_stop_the_others_and_the_block_is_not_written() {
    let p = Project::new(AB);
    p.source.fail("a", Method::Materialise, "no space left");

    let report = p.sync(&SyncFlags::default());

    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(codes(&report), ["refs::git::failed"]);
    assert!(matches!(
        p.source.inspect("b").unwrap(),
        Observed::At { .. }
    ));
    assert_eq!(p.read("AGENTS.md"), None);
    assert!(p.lock().is_some());
}

#[test]
fn a_failed_stage_one_writes_no_lock_and_touches_no_checkout() {
    let p = Project::new(AB);
    p.source.fail("a", Method::Resolve, "unreachable");
    p.source.fail("b", Method::Verify, "no such path");

    let report = p.sync(&SyncFlags::default());

    assert_eq!(report.outcome, Outcome::Failed);
    // every error is collected: the resolve of a and the verify of b
    assert_eq!(codes(&report).len(), 2);
    assert_eq!(report.drift, [Drift::LockMissing]);
    assert_eq!(p.lock(), None);
    assert!(
        !p.source
            .calls()
            .iter()
            .any(|c| matches!(c, Call::Materialise { .. } | Call::Remove(_)))
    );
}

#[test]
fn a_removed_repo_whose_sync_failed_is_still_removed_by_the_retry() {
    let mut p = synced(AB);
    p.config = parse(&AB.replace("[repos.b]", "[repos.b]\nenabled = false")).unwrap();
    p.source.fail("b", Method::Remove, "busy");

    assert_eq!(p.sync(&SyncFlags::default()).outcome, Outcome::Failed);
    // stage 1 already dropped b from the Lock, so only the directory listing remembers it
    assert_eq!(p.lock().unwrap().repo.len(), 1);
    assert!(matches!(
        p.source.inspect("b").unwrap(),
        Observed::At { .. }
    ));

    p.source.heal("b", Method::Remove);
    assert_eq!(p.sync(&SyncFlags::default()).outcome, Outcome::InSync);
    assert_eq!(p.source.inspect("b").unwrap(), Observed::Absent);
}

#[test]
fn check_reports_a_stale_lock_without_resolving() {
    let mut p = synced(A);
    let seen = p.source.calls().len();
    assert_eq!(p.sync(&check()).outcome, Outcome::InSync);
    assert_eq!(p.source.calls().len(), seen);
    p.config = parse(&format!(
        "{A}\n[repos.b]\nurl = \"https://github.com/o/b\"\n"
    ))
    .unwrap();
    let seen = p.source.calls().len();

    let report = p.sync(&check());

    assert_eq!(report.outcome, Outcome::OutOfDate);
    assert_eq!(report.drift, [refs_cli::plan::Drift::Added("b".into())]);
    assert_eq!(p.source.calls().len(), seen);
}

#[test]
fn check_tells_drift_from_a_refusal() {
    let p = synced(A);
    p.source.seed("a", Observed::Foreign);
    let report = p.sync(&check());
    assert_eq!(report.outcome, Outcome::Refused);
    assert_eq!(codes(&report), ["refs::sync::foreign_dir"]);

    let p = synced(A);
    fs::write(p.path("AGENTS.md"), "<!-- BEGIN:refs -->\n").unwrap();
    let report = p.sync(&check());
    assert_eq!(report.outcome, Outcome::Refused);
    assert_eq!(codes(&report), ["refs::sync::bad_markers"]);

    let p = synced(A);
    fs::write(p.path("AGENTS.md"), "mine\n").unwrap();
    assert_eq!(p.sync(&check()).outcome, Outcome::OutOfDate);
}

/// Sync the project's source with `config` instead of the one it was made with.
fn sync_with(p: &Project, config: &str, flags: &SyncFlags) -> Report {
    sync(&p.source, p.dir.path(), &parse(config).unwrap(), flags)
}

#[test]
fn a_paths_only_edit_leaves_refs_lock_alone_but_changes_the_checkout() {
    let p = synced(A);
    let lock = p.read("refs.lock").unwrap();
    let wider = A.replace(r#"["docs"]"#, r#"["docs", "src"]"#);

    assert_eq!(sync_with(&p, &wider, &check()).outcome, Outcome::OutOfDate);
    let report = sync_with(&p, &wider, &SyncFlags::default());

    assert_eq!(report.outcome, Outcome::InSync, "{:?}", report.diagnostics);
    assert_eq!(p.read("refs.lock").unwrap(), lock);
    assert!(!lock.contains("paths"));
    let Observed::At { paths, .. } = p.source.inspect("a").unwrap() else {
        panic!("not checked out")
    };
    assert_eq!(paths, ["docs", "src"]);
}

#[test]
fn offline_refuses_a_missing_lock_and_never_calls_the_source() {
    let p = Project::new(A);
    let report = p.sync(&SyncFlags {
        offline: true,
        ..SyncFlags::default()
    });
    assert_eq!(report.outcome, Outcome::Refused);
    assert_eq!(codes(&report), ["refs::lock::offline_stale"]);
    assert_eq!(p.source.calls(), []);
}

#[test]
fn offline_is_passed_to_materialise_and_verify() {
    let p = Project::new(A);
    assert_eq!(
        lock(&p.source, p.dir.path(), &p.config, &LockFlags::default()).outcome,
        Outcome::InSync
    );
    let seen = p.source.calls().len();
    let offline = SyncFlags {
        offline: true,
        ..SyncFlags::default()
    };
    assert_eq!(p.sync(&offline).outcome, Outcome::InSync);
    let calls = &p.source.calls()[seen..];
    assert!(calls.contains(&Call::Materialise {
        id: "a".into(),
        offline: true
    }));
    assert!(calls.contains(&Call::Verify {
        id: "a".into(),
        offline: true
    }));
}

#[test]
fn a_project_that_is_not_a_git_repo_continues_with_a_note() {
    let p = Project::new(A);
    fs::remove_dir_all(p.path(".git")).unwrap();
    let report = p.sync(&SyncFlags::default());
    assert_eq!(report.outcome, Outcome::InSync);
    assert_eq!(codes(&report), ["refs::sync::no_git_repo"]);
    assert!(p.read("AGENTS.md").is_some());
}

#[test]
fn a_failed_removal_of_an_inactive_repo_does_not_hold_back_the_block() {
    let mut p = synced(AB);
    assert!(p.read("AGENTS.md").unwrap().contains("[b @"));
    p.config = parse(&AB.replace("[repos.b]", "[repos.b]\nenabled = false")).unwrap();
    p.source.fail("b", Method::Remove, "busy");

    let report = p.sync(&SyncFlags::default());

    // still a failure to report, but the block lists only what has a checkout
    assert_eq!(report.outcome, Outcome::Failed);
    let block = p.read("AGENTS.md").unwrap();
    assert!(block.contains("[a @"), "{block}");
    assert!(!block.contains("[b @"), "{block}");
}

#[cfg(unix)]
#[test]
fn a_failed_agent_file_write_does_not_stop_the_next_file() {
    use std::os::unix::fs::PermissionsExt;

    let p = Project::new(&format!(
        "[settings]\nagents_files = [\"locked/AGENTS.md\", \"AGENTS.md\"]\n{A}"
    ));
    fs::create_dir(p.path("locked")).unwrap();
    fs::set_permissions(p.path("locked"), fs::Permissions::from_mode(0o555)).unwrap();

    let report = p.sync(&SyncFlags::default());

    fs::set_permissions(p.path("locked"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(codes(&report), ["refs::io::write_failed"]);
    assert!(p.read("AGENTS.md").unwrap().contains("<!-- BEGIN:refs -->"));
}

#[test]
fn a_failed_replace_reports_no_recreated_note() {
    let p = synced(A);
    p.source.seed("a", Observed::Dangling);
    p.source.fail("a", Method::Materialise, "no space left");

    let report = p.sync(&SyncFlags::default());

    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(codes(&report), ["refs::git::failed"]);
}

const NO_REPOS: &str = "[settings]\n";

#[test]
fn disabling_the_last_repo_removes_the_block_and_keeps_the_rest_of_the_file() {
    let mut p = synced(A);
    let block = p.read("AGENTS.md").unwrap();
    fs::write(p.path("AGENTS.md"), format!("# mine\n\n{block}\ntail\n")).unwrap();
    fs::write(p.path("CLAUDE.md"), "no markers here\n").unwrap();
    p.config = parse(&A.replace("[repos.a]", "[repos.a]\nenabled = false")).unwrap();

    assert_eq!(p.sync(&check()).outcome, Outcome::OutOfDate);

    assert_eq!(p.sync(&SyncFlags::default()).outcome, Outcome::InSync);
    let left = p.read("AGENTS.md").unwrap();
    assert!(!left.contains("refs"), "{left}");
    assert!(left.starts_with("# mine\n\n") && left.ends_with("\ntail\n"));
    assert_eq!(p.read("CLAUDE.md").unwrap(), "no markers here\n");
    assert_eq!(p.sync(&check()).outcome, Outcome::InSync);
}

#[test]
fn a_file_left_empty_by_removing_the_block_is_kept() {
    let mut p = synced(A);
    fs::write(p.path("AGENTS.md"), p.read("AGENTS.md").unwrap().trim_end()).unwrap();
    p.config = parse(NO_REPOS).unwrap();
    assert_eq!(p.sync(&SyncFlags::default()).outcome, Outcome::InSync);
    assert_eq!(p.read("AGENTS.md").as_deref(), Some(""));
}
