use refs_cli::active::{ActiveSet, active};
use refs_cli::config::{Config, parse};
use refs_cli::lock::{Lock, LockedRepo};
use refs_cli::plan::{Coverage, Failure, Keep, Settled, settle};
use refs_cli::source::Pin;

const SHA: &str = "ee49b3e0123456789012345678901234567890ab";

const ABC: &str = "[repos.a]\nurl = \"https://github.com/o/a\"\n\n[repos.b]\nurl = \"https://github.com/o/b\"\n\n[repos.c]\nurl = \"https://github.com/o/c\"\n";

fn config() -> Config {
    parse(ABC).unwrap()
}

fn entry(id: &str) -> LockedRepo {
    LockedRepo {
        id: id.into(),
        pin: Pin::git(&format!("https://github.com/o/{id}"), "HEAD", SHA, None),
    }
}

fn lock(ids: &[&str]) -> Lock {
    Lock::new(ids.iter().map(|id| entry(id)).collect())
}

fn failure(id: &str) -> Failure {
    Failure {
        id: id.into(),
        error: miette::miette!("{id} failed"),
    }
}

fn run<'a>(
    set: &ActiveSet<'a>,
    old: Option<&Lock>,
    passed: &[&str],
    failed: &[&str],
    keep: Keep,
) -> Settled<'a> {
    settle(
        set,
        old,
        passed.iter().map(|id| entry(id)).collect(),
        failed.iter().map(|id| failure(id)).collect(),
        keep,
    )
}

fn ids(set: &ActiveSet) -> Vec<String> {
    set.repos().map(|r| r.id.to_string()).collect()
}

fn covered(coverage: &Coverage) -> Vec<String> {
    ids(&coverage.active)
}

#[test]
fn all_or_nothing_with_a_failure_writes_nothing_and_rejects_the_edit() {
    let config = config();
    let set = active(&config);

    let s = run(&set, None, &["a", "c"], &["b"], Keep::AllOrNothing);

    assert!(!s.write_lock);
    assert!(!s.accepted);
    assert_eq!(s.errors.len(), 1);
}

#[test]
fn all_or_nothing_without_a_failure_writes_the_lock_and_accepts() {
    let config = config();
    let set = active(&config);

    let s = run(&set, None, &["a", "b", "c"], &[], Keep::AllOrNothing);

    assert!(s.write_lock);
    assert!(s.accepted);
    assert!(s.errors.is_empty());
    assert_eq!(s.lock, lock(&["a", "b", "c"]));
}

#[test]
fn passing_with_failures_writes_the_entries_that_passed_and_accepts() {
    let config = config();
    let set = active(&config);

    let s = run(&set, None, &["a", "c"], &["b"], Keep::Passing);

    assert!(s.write_lock);
    assert!(s.accepted);
    assert_eq!(s.lock, lock(&["a", "c"]));
    assert_eq!(s.errors.len(), 1, "the failure is still reported");
}

#[test]
fn coverage_and_withheld_follow_the_failures() {
    let config = config();
    let set = active(&config);

    let s = run(&set, None, &["a", "c"], &["b"], Keep::Passing);

    assert_eq!(covered(&s.coverage), ["a", "c"]);
    assert_eq!(s.coverage.withheld, ["b"]);
}

#[test]
fn with_no_failure_everything_is_covered_and_nothing_withheld() {
    let config = config();
    let set = active(&config);

    let s = run(&set, None, &["a", "b", "c"], &[], Keep::Passing);

    assert_eq!(covered(&s.coverage), ["a", "b", "c"]);
    assert!(s.coverage.withheld.is_empty());
}

#[test]
fn an_unchanged_lock_is_not_rewritten() {
    let config = config();
    let set = active(&config);
    let old = lock(&["a", "b", "c"]);

    let s = run(&set, Some(&old), &["a", "b", "c"], &[], Keep::AllOrNothing);

    assert!(!s.write_lock);
    assert!(s.accepted);
}

#[test]
fn a_lock_that_only_lost_entries_is_rewritten_under_passing() {
    let config = config();
    let set = active(&config);
    let old = lock(&["a", "b", "c"]);

    let s = run(&set, Some(&old), &["a", "c"], &["b"], Keep::Passing);

    assert!(s.write_lock);
}

#[test]
fn every_repo_failing_under_passing_leaves_an_empty_lock_and_withholds_all() {
    let config = config();
    let set = active(&config);
    let old = lock(&["a", "b", "c"]);

    let s = run(&set, Some(&old), &[], &["a", "b", "c"], Keep::Passing);

    assert!(s.accepted);
    assert!(s.write_lock);
    assert_eq!(s.lock, lock(&[]));
    assert!(covered(&s.coverage).is_empty());
    assert_eq!(s.coverage.withheld, ["a", "b", "c"]);
}

#[test]
fn errors_keep_the_order_the_failures_came_in() {
    let config = config();
    let set = active(&config);

    let s = run(&set, None, &["b"], &["c", "a"], Keep::Passing);

    let messages: Vec<String> = s.errors.iter().map(|e| e.to_string()).collect();
    assert_eq!(messages, ["c failed", "a failed"]);
    assert_eq!(s.coverage.withheld, ["c", "a"]);
}
