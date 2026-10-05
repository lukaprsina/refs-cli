//! `list --status` as the CLI makes it: the Lock read from the project root and the Checkouts
//! inspected through a `Source`, then rendered.

use refs_cli::config::parse;
use refs_cli::list::list_status;
use refs_cli::lock::{Lock, LockedRepo};
use refs_cli::source::fake::{FakeSource, Method};
use refs_cli::source::{Observed, Pin};

fn pin(sha: &str) -> Pin {
    Pin::git("https://github.com/o/r", "HEAD", sha, None)
}

fn status_listing(source: &FakeSource, lock: Lock, config: &str) -> String {
    let config = parse(config).unwrap();
    let root = tempfile::tempdir().unwrap();
    lock.write(&Lock::path(root.path())).unwrap();
    let status = refs_cli::sync::status(source, root.path(), &config).unwrap();
    list_status(&config, &status, false)
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

#[test]
fn status_says_dirty_where_sync_would_refuse_to_move_the_checkout() {
    let sha = "1".repeat(40);
    let source = FakeSource::new();
    source.seed("moved", seen(&"2".repeat(40), &[], &["x"]));
    source.seed("paths", seen(&sha, &["b"], &["x"]));
    source.seed("same", seen(&sha, &[], &["x"]));
    let out = status_listing(
        &source,
        locked_at(&sha, &["moved", "paths", "same"]),
        TWO_PATHS,
    );
    assert!(out.contains("1111111  wrong SHA, dirty\n"), "{out}");
    assert!(out.contains("1111111  wrong paths, dirty\n"), "{out}");
    // dirty but matching: sync leaves it alone
    assert!(out.contains("1111111  ok\n"), "{out}");
}

#[test]
fn status_never_inspects_a_disabled_repo() {
    let source = FakeSource::new();
    source.fail("off", Method::Inspect, "broken");
    let out = status_listing(
        &source,
        locked_at(&"1".repeat(40), &["off"]),
        "[repos.off]\nurl = \"https://github.com/o/r\"\nenabled = false\n",
    );
    assert!(out.contains("disabled\n"), "{out}");
}
