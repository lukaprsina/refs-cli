//! What each command says, and where: status lines on stderr, data on stdout, `-q`, hints
//! and exit codes. `cli::run_with` takes the two streams, so the in-process tests (against
//! the fake `Source`) assert stdout and stderr separately; the binary tests below do the
//! same through a real process for what needs no `Source`.

mod common;

use std::fs;

use common::git;

use refs_cli::cli::run_with;
use refs_cli::source::Observed;
use refs_cli::source::fake::{FakeSource, Method};
use tempfile::TempDir;

const AB: &str = r#"
[repos.a]
url = "https://github.com/o/a"
[repos.b]
url = "https://github.com/o/b"
"#;

struct Project {
    dir: TempDir,
    source: FakeSource,
}

#[derive(Debug)]
struct Ran {
    code: u8,
    stdout: String,
    stderr: String,
}

impl Project {
    fn new(config: &str) -> Project {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q"]);
        fs::write(dir.path().join("refs.toml"), config).unwrap();
        Project {
            dir,
            source: FakeSource::new(),
        }
    }

    fn run(&self, args: &[&str]) -> Ran {
        let args = std::iter::once("refs").chain(args.iter().copied());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(
            args.map(Into::into),
            self.dir.path(),
            |_, _| Ok(Box::new(&self.source)),
            &mut out,
            &mut err,
        );
        Ran {
            code,
            stdout: String::from_utf8(out).unwrap(),
            stderr: String::from_utf8(err).unwrap(),
        }
    }
}

#[test]
fn check_keeps_exit_3_for_out_of_date_and_1_for_refusals_and_prints_no_hint() {
    let p = Project::new(AB);

    let stale = p.run(&["sync", "--check"]);
    assert_eq!(stale.code, 3);
    assert_eq!(stale.stdout, "");
    assert!(stale.stderr.contains("out of date"), "{}", stale.stderr);
    assert!(
        !stale.stderr.contains("run `refs sync`"),
        "{}",
        stale.stderr
    );

    p.run(&["sync"]);
    let fresh = p.run(&["sync", "--check"]);
    assert_eq!((fresh.code, fresh.stdout.as_str()), (0, ""));
    assert_eq!(fresh.stderr, "up to date\n");

    p.source.seed("a", Observed::Foreign);
    assert_eq!(p.run(&["sync", "--check"]).code, 1);
}

#[test]
fn list_is_data_on_stdout_and_nothing_on_stderr() {
    let p = Project::new(AB);

    let ran = p.run(&["list"]);

    assert_eq!(ran.code, 0);
    assert!(
        ran.stdout.contains("https://github.com/o/a"),
        "{}",
        ran.stdout
    );
    assert_eq!(ran.stderr, "");
}

#[test]
fn removing_is_not_blocked_by_another_repo_that_no_longer_verifies() {
    let p = Project::new(&format!(
        "{AB}[repos.c]
url = \"https://github.com/o/c\"
"
    ));
    assert_eq!(p.run(&["sync"]).code, 0);
    p.source.fail("a", Method::Verify, "remote gone");
    p.source.fail("b", Method::Verify, "remote gone");

    let ran = p.run(&["remove", "c"]);

    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let block = fs::read_to_string(p.dir.path().join("AGENTS.md")).unwrap();
    assert!(block.contains("[a @") && !block.contains("[c @"), "{block}");
}

const RUN_SYNC: &str = "run `refs sync` to bring the project up to date";
const ONCE_FIXED: &str = "run `refs sync` once the problem is fixed";
const FIX_OR_REMOVE: &str =
    "refs.toml was updated; fix or remove the broken repo, then run `refs sync`";
const NOT_CHANGED: &str =
    "refs.toml was not changed; fix the broken repo, or pass `--no-sync` to edit without syncing";

/// The hint is the last line stderr ends with, and it is printed under `-q` too; stdout stays
/// data only.
fn last_line(ran: &Ran) -> &str {
    assert_eq!(ran.stdout, "");
    ran.stderr.lines().last().unwrap_or_default()
}

#[test]
fn an_edit_with_no_sync_says_to_run_sync_even_under_q() {
    for args in [
        &["add", "https://github.com/o/c", "--no-sync"][..],
        &["-q", "add", "https://github.com/o/c", "--no-sync"][..],
    ] {
        let p = Project::new(AB);

        let ran = p.run(args);

        assert_eq!(ran.code, 0, "{}", ran.stderr);
        assert_eq!(last_line(&ran), RUN_SYNC, "{}", ran.stderr);
    }
}

#[test]
fn a_sync_that_failed_or_was_refused_says_to_run_sync_once_fixed_even_under_q() {
    let failed = Project::new(AB);
    failed.source.fail("a", Method::Resolve, "remote gone");
    let refused = Project::new(AB);
    refused.source.seed("a", Observed::Foreign);

    for (p, args) in [
        (&failed, &["sync"][..]),
        (&failed, &["-q", "sync"][..]),
        (&refused, &["sync"][..]),
        (&refused, &["sync", "--check"][..]),
    ] {
        let ran = p.run(args);

        assert_eq!(ran.code, 1, "{}", ran.stderr);
        assert_eq!(last_line(&ran), ONCE_FIXED, "{}", ran.stderr);
    }
}

#[test]
fn an_edit_written_but_then_failed_says_fix_or_remove_the_broken_repo() {
    let p = Project::new(AB);
    assert_eq!(p.run(&["sync"]).code, 0);
    p.source.fail("c", Method::Materialise, "disk full");

    let ran = p.run(&["add", "https://github.com/o/c"]);

    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert_eq!(last_line(&ran), FIX_OR_REMOVE, "{}", ran.stderr);
    assert!(
        fs::read_to_string(p.dir.path().join("refs.toml"))
            .unwrap()
            .contains("o/c")
    );
}

#[test]
fn a_rejected_edit_says_refs_toml_was_not_changed() {
    let p = Project::new(AB);
    p.source.fail("c", Method::Resolve, "remote gone");

    let ran = p.run(&["add", "https://github.com/o/c"]);

    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert_eq!(last_line(&ran), NOT_CHANGED, "{}", ran.stderr);
}

#[test]
fn an_edit_that_changes_nothing_and_then_fails_says_to_run_sync_once_fixed() {
    let p = Project::new(AB);
    p.source.fail("a", Method::Resolve, "remote gone");

    let ran = p.run(&["enable", "a"]);

    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert_eq!(last_line(&ran), ONCE_FIXED, "{}", ran.stderr);
}

#[test]
fn a_run_that_left_nothing_to_fix_prints_no_sync_hint() {
    let p = Project::new(AB);
    let failed_lock = Project::new(AB);
    failed_lock.source.fail("a", Method::Resolve, "remote gone");

    let ok = p.run(&["sync"]);
    let again = p.run(&["-q", "sync"]);
    let lock = p.run(&["lock"]);
    let stale = Project::new(AB).run(&["sync", "--check"]);
    let lock_failed = failed_lock.run(&["lock"]);

    for ran in [&ok, &again, &lock, &stale, &lock_failed] {
        assert!(!ran.stderr.contains("run `refs sync`"), "{}", ran.stderr);
    }
    assert_eq!(
        (ok.code, again.code, lock.code, stale.code, lock_failed.code),
        (0, 0, 0, 3, 1)
    );
}

struct Binary {
    dir: TempDir,
    cache: TempDir,
}

impl Binary {
    fn new() -> Binary {
        Binary {
            dir: TempDir::new().unwrap(),
            cache: TempDir::new().unwrap(),
        }
    }

    fn run(&self, args: &[&str]) -> Ran {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_refs"))
            .args(args)
            .arg("--project")
            .arg(self.dir.path())
            .env("XDG_CACHE_HOME", self.cache.path())
            .output()
            .unwrap();
        Ran {
            code: u8::try_from(out.status.code().unwrap()).unwrap(),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8(out.stderr).unwrap(),
        }
    }
}

#[test]
fn the_binary_init_reports_on_stderr_without_a_sync_hint_and_a_second_run_changes_nothing() {
    let b = Binary::new();
    git(b.dir.path(), &["init", "-q"]);

    let first = b.run(&["init", "--here"]);

    assert_eq!(first.code, 0);
    assert_eq!(first.stdout, "");
    assert!(
        first.stderr.starts_with("created refs.toml\n"),
        "{}",
        first.stderr
    );
    assert!(
        !first.stderr.contains("updated AGENTS.md"),
        "{}",
        first.stderr
    );
    assert!(first.stderr.contains("refs add <url>"), "{}", first.stderr);
    assert!(!first.stderr.contains("refs sync"), "{}", first.stderr);
    let template = fs::read_to_string(b.dir.path().join("refs.toml")).unwrap();
    assert!(!template.contains("refs sync"), "{template}");

    let second = b.run(&["init", "--here"]);
    assert_eq!(second.code, 0);
    assert_eq!(second.stdout, "");
    assert!(
        second.stderr.starts_with("nothing changed\n"),
        "{}",
        second.stderr
    );
}
