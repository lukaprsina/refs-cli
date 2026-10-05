//! `status::build`: the Lock read from the project root and the Checkouts inspected through
//! a `Source`, as one row per Repo.

use refs_cli::config::parse;
use refs_cli::lock::{Lock, LockedRepo};
use refs_cli::plan::{Cause, CheckoutState};
use refs_cli::source::fake::{FakeSource, Method};
use refs_cli::source::{Observed, Pin};
use refs_cli::status::{Kind, Row, Status, build};

fn pin(sha: &str) -> Pin {
    Pin::git("https://github.com/o/r", "HEAD", sha, None)
}

fn status_of(source: &FakeSource, lock: Option<Lock>, config: &str) -> Status {
    let config = parse(config).unwrap();
    let root = tempfile::tempdir().unwrap();
    if let Some(lock) = lock {
        lock.write(&Lock::path(root.path())).unwrap();
    }
    build(source, root.path(), &config).unwrap()
}

const TWO_PATHS: &str = r#"
[repos.moved]
url = "https://github.com/o/r"
[repos.paths]
url = "https://github.com/o/r"
paths = ["a"]
[repos.same]
url = "https://github.com/o/r"
"#;

fn locked_at(sha: &str, ids: &[&str]) -> Lock {
    Lock::new(
        ids.iter()
            .map(|id| LockedRepo {
                id: (*id).into(),
                pin: pin(sha),
            })
            .collect(),
    )
}

fn seen(sha: &str, paths: &[&str], dirty: &[&str]) -> Observed {
    Observed::At {
        pin: pin(sha),
        paths: paths.iter().map(|p| p.to_string()).collect(),
        dirty_files: dirty.iter().map(|p| p.to_string()).collect(),
    }
}

fn stale(cause: Cause, dirty: &[&str]) -> Kind {
    Kind::Checkout(CheckoutState::Stale {
        cause,
        dirty_files: dirty.iter().map(|p| p.to_string()).collect(),
    })
}

#[test]
fn a_dirty_checkout_is_stale_with_its_dirty_files_and_a_matching_one_is_in_sync() {
    let sha = "1".repeat(40);
    let source = FakeSource::new();
    source.seed("moved", seen(&"2".repeat(40), &[], &["x"]));
    source.seed("paths", seen(&sha, &["b"], &["x"]));
    source.seed("same", seen(&sha, &[], &["x"]));
    let status = status_of(
        &source,
        Some(locked_at(&sha, &["moved", "paths", "same"])),
        TWO_PATHS,
    );
    assert_eq!(status.rows["moved"].kind, stale(Cause::Commit, &["x"]));
    assert_eq!(status.rows["paths"].kind, stale(Cause::Paths, &["x"]));
    // dirty but matching: sync leaves it alone
    assert_eq!(
        status.rows["same"],
        Row {
            sha: Some("1111111".into()),
            kind: Kind::Checkout(CheckoutState::InSync)
        }
    );
}

#[test]
fn a_locked_disabled_repo_is_disabled_and_never_inspected() {
    let source = FakeSource::new();
    source.fail("off", Method::Inspect, "broken");
    let status = status_of(
        &source,
        Some(locked_at(&"1".repeat(40), &["off"])),
        "[repos.off]\nurl = \"https://github.com/o/r\"\nenabled = false\n",
    );
    assert_eq!(
        status.rows["off"],
        Row {
            sha: Some("1111111".into()),
            kind: Kind::Disabled
        }
    );
}

#[test]
fn an_unlocked_repo_is_not_locked_with_or_without_a_lock_file() {
    let config = "[repos.a]\nurl = \"https://github.com/o/r\"\n";
    let source = FakeSource::new();
    source.seed("a", seen(&"1".repeat(40), &[], &[]));
    for lock in [None, Some(locked_at(&"1".repeat(40), &["other"]))] {
        let status = status_of(&source, lock, config);
        assert_eq!(
            status.rows["a"],
            Row {
                sha: None,
                kind: Kind::NotLocked
            }
        );
    }
}

#[test]
fn every_failing_inspect_is_reported() {
    let source = FakeSource::new();
    source.fail("a", Method::Inspect, "broken");
    source.fail("b", Method::Inspect, "broken");
    let config = parse(
        "[repos.a]\nurl = \"https://github.com/o/r\"\n[repos.b]\nurl = \"https://github.com/o/r\"\n",
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let errors = build(&source, root.path(), &config).unwrap_err();
    assert_eq!(errors.len(), 2);
}
