//! `refs list --status`, through `cli::run_with` against the fake `Source`: the Lock read from
//! the project root and the Checkouts inspected through the `Source`, as one line per Repo.
//! `status::build` is not a seam of its own: only the CLI calls it.

use std::fs;

use refs_cli::cli::run_with;
use refs_cli::lock::{Lock, LockedRepo};
use refs_cli::source::fake::{FakeSource, Method};
use refs_cli::source::{Observed, Pin};

fn pin(sha: &str) -> Pin {
    Pin::git("https://github.com/o/r", "HEAD", sha, None)
}

/// Run `refs list --status` over `config` and the optional `lock`: exit code, stdout, stderr.
fn list_status(source: &FakeSource, lock: Option<Lock>, config: &str) -> (u8, String, String) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("refs.toml"), config).unwrap();
    if let Some(lock) = lock {
        lock.write(&Lock::path(dir.path())).unwrap();
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["refs", "list", "--status", "--no-color"].map(Into::into);
    let code = run_with(
        args,
        dir.path(),
        |_, _| Ok(Box::new(source)),
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// The last two cells of each line: the short SHA and the state.
fn states(stdout: &str) -> Vec<(String, String)> {
    stdout
        .lines()
        .map(|line| {
            let cells: Vec<&str> = line.split("  ").filter(|c| !c.is_empty()).collect();
            let id = cells[0].trim_start_matches("- ").trim().to_string();
            (id, cells[cells.len() - 2..].join("|").trim().to_string())
        })
        .collect()
}

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

const TWO_PATHS: &str = r#"
[repos.moved]
url = "https://github.com/o/r"
[repos.paths]
url = "https://github.com/o/r"
paths = ["a"]
[repos.same]
url = "https://github.com/o/r"
"#;

#[test]
fn a_dirty_checkout_is_stale_and_a_matching_one_is_in_sync() {
    let sha = "1".repeat(40);
    let source = FakeSource::new();
    source.seed("moved", seen(&"2".repeat(40), &[], &["x"]));
    source.seed("paths", seen(&sha, &["b"], &["x"]));
    source.seed("same", seen(&sha, &[], &["x"]));

    let (code, stdout, _) = list_status(
        &source,
        Some(locked_at(&sha, &["moved", "paths", "same"])),
        TWO_PATHS,
    );

    assert_eq!(code, 0);
    // "same" is dirty but matching: sync leaves it alone, so it is ok
    assert_eq!(
        states(&stdout),
        [
            ("moved".to_string(), "1111111|wrong SHA, dirty".to_string()),
            (
                "paths".to_string(),
                "1111111|wrong paths, dirty".to_string()
            ),
            ("same".to_string(), "1111111|ok".to_string()),
        ]
    );
}

#[test]
fn a_locked_disabled_repo_is_disabled_and_never_inspected() {
    let source = FakeSource::new();
    source.fail("off", Method::Inspect, "broken");

    let (code, stdout, stderr) = list_status(
        &source,
        Some(locked_at(&"1".repeat(40), &["off"])),
        "[repos.off]
url = \"https://github.com/o/r\"
enabled = false
",
    );

    assert_eq!((code, stderr.as_str()), (0, ""));
    assert_eq!(
        states(&stdout),
        [("off".to_string(), "1111111|disabled".to_string())]
    );
}

#[test]
fn an_unlocked_repo_is_not_locked_with_or_without_a_lock_file() {
    let config = "[repos.a]
url = \"https://github.com/o/r\"
";
    let source = FakeSource::new();
    source.seed("a", seen(&"1".repeat(40), &[], &[]));
    for lock in [None, Some(locked_at(&"1".repeat(40), &["other"]))] {
        let (code, stdout, _) = list_status(&source, lock, config);
        assert_eq!(code, 0);
        assert_eq!(
            states(&stdout),
            [("a".to_string(), "-|not locked".to_string())]
        );
    }
}

#[test]
fn every_failing_inspect_is_reported_by_repo_and_the_listing_fails() {
    let source = FakeSource::new();
    source.fail("a", Method::Inspect, "a is broken");
    source.fail("b", Method::Inspect, "b is broken");
    let config = "[repos.a]
url = \"https://github.com/o/r\"
[repos.b]
url = \"https://github.com/o/r\"
[repos.c]
url = \"https://github.com/o/r\"
";

    let (code, stdout, stderr) = list_status(&source, None, config);

    assert_eq!(code, 1, "{stderr}");
    assert_eq!(stdout, "");
    assert!(stderr.contains("repo `a`: a is broken"), "{stderr}");
    assert!(stderr.contains("repo `b`: b is broken"), "{stderr}");
    assert!(!stderr.contains("repo `c`"), "{stderr}");
}
