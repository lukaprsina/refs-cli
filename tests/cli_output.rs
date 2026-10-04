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
fn an_in_sync_sync_says_so_in_one_line_on_stderr() {
    let p = Project::new(AB);
    p.run(&["sync"]);

    let ran = p.run(&["sync"]);

    assert_eq!(ran.code, 0);
    assert_eq!(ran.stdout, "");
    assert_eq!(ran.stderr, "nothing changed\n");
}

#[test]
fn a_first_sync_names_the_lock_each_checkout_and_each_agent_file() {
    let p = Project::new(AB);

    let ran = p.run(&["sync"]);

    assert_eq!(ran.code, 0);
    assert_eq!(ran.stdout, "");
    assert_eq!(
        ran.stderr,
        "updated refs.lock\ncreated checkout `a`\ncreated checkout `b`\nupdated AGENTS.md\n"
    );
}

#[test]
fn lock_says_it_updated_the_lock_and_then_that_nothing_changed() {
    let p = Project::new(AB);

    let first = p.run(&["lock"]);
    let second = p.run(&["lock"]);

    assert_eq!((first.code, first.stdout.as_str()), (0, ""));
    assert_eq!(first.stderr, "updated refs.lock\n");
    assert_eq!((second.code, second.stdout.as_str()), (0, ""));
    assert_eq!(second.stderr, "nothing changed\n");
}

#[test]
fn quiet_hides_status_lines_but_not_problems() {
    let p = Project::new(AB);
    let ran = p.run(&["sync", "-q"]);
    assert_eq!(
        (ran.code, ran.stdout, ran.stderr),
        (0, "".into(), "".into())
    );
    assert_eq!(p.run(&["sync", "-q"]).stderr, "");

    p.source.seed("a", Observed::Foreign);
    let ran = p.run(&["sync", "-q"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.stdout, "");
    assert!(ran.stderr.contains("refs::sync::foreign"), "{}", ran.stderr);
}

#[test]
fn a_failed_sync_ends_with_the_hint_to_run_sync_and_a_good_one_has_none() {
    let p = Project::new(AB);
    p.source.fail("a", Method::Materialise, "no space left");

    let failed = p.run(&["sync"]);

    assert_eq!(failed.code, 1);
    assert_eq!(failed.stdout, "");
    assert!(failed.stderr.contains("no space left"), "{}", failed.stderr);
    assert!(
        failed
            .stderr
            .ends_with("run `refs sync` once the problem is fixed\n"),
        "{}",
        failed.stderr
    );
    p.source.heal("a", Method::Materialise);
    let ok = p.run(&["sync"]);
    assert_eq!(ok.code, 0);
    assert!(!ok.stderr.contains("run `refs sync`"), "{}", ok.stderr);
}

#[test]
fn add_says_what_it_changed_and_hints_only_with_no_sync() {
    let p = Project::new(AB);
    p.run(&["sync"]);

    let synced = p.run(&["add", "https://github.com/o/c", "--group", "extra"]);
    assert_eq!(synced.code, 0);
    assert_eq!(synced.stdout, "");
    assert_eq!(
        synced.stderr,
        "updated refs.toml\ncreated group `extra`\nupdated refs.lock\n\
         created checkout `c`\nupdated AGENTS.md\n"
    );

    let unsynced = p.run(&["add", "https://github.com/o/d", "--no-sync"]);
    assert_eq!(unsynced.code, 0);
    assert_eq!(unsynced.stdout, "");
    assert_eq!(
        unsynced.stderr,
        "updated refs.toml\nrun `refs sync` to bring the project up to date\n"
    );
}

#[test]
fn remove_and_disable_say_which_checkout_went() {
    let p = Project::new(AB);
    p.run(&["sync"]);

    let disabled = p.run(&["disable", "a"]);
    assert_eq!(disabled.stdout, "");
    assert_eq!(
        disabled.stderr,
        "updated refs.toml\nupdated refs.lock\nremoved checkout `a`\nupdated AGENTS.md\n"
    );

    let removed = p.run(&["remove", "b"]);
    assert_eq!(removed.stdout, "");
    assert_eq!(
        removed.stderr,
        "updated refs.toml\nupdated refs.lock\nremoved checkout `b`\nupdated AGENTS.md\n"
    );
}

#[test]
fn a_changed_ref_moves_the_checkout_and_says_so() {
    let p = Project::new(AB);
    p.run(&["sync"]);
    let config = fs::read_to_string(p.dir.path().join("refs.toml")).unwrap();
    let moved = config.replace(
        "url = \"https://github.com/o/a\"",
        "url = \"https://github.com/o/a\"\nref = \"next\"",
    );
    fs::write(p.dir.path().join("refs.toml"), moved).unwrap();

    let ran = p.run(&["sync"]);

    assert_eq!(ran.code, 0);
    assert_eq!(
        ran.stderr,
        "updated refs.lock\nmoved checkout `a`\nupdated AGENTS.md\n"
    );
}

#[test]
fn an_edit_that_changes_nothing_says_so() {
    let p = Project::new(AB);
    p.run(&["sync"]);

    let ran = p.run(&["enable", "a"]);

    assert_eq!(ran.code, 0);
    assert_eq!(ran.stdout, "");
    assert_eq!(ran.stderr, "nothing changed\n");
}

#[test]
fn an_edit_whose_sync_fails_keeps_refs_toml_and_hints_to_sync() {
    let p = Project::new(AB);
    p.run(&["sync"]);
    p.source.fail("c", Method::Materialise, "network down");

    let ran = p.run(&["add", "https://github.com/o/c"]);

    assert_eq!(ran.code, 1);
    assert_eq!(ran.stdout, "");
    assert!(
        ran.stderr.starts_with("updated refs.toml\n"),
        "{}",
        ran.stderr
    );
    assert!(
        ran.stderr
            .ends_with("refs.toml was updated; run `refs sync` once the problem is fixed\n"),
        "{}",
        ran.stderr
    );
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
        first.stderr.contains("updated AGENTS.md"),
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

#[test]
fn the_binary_sync_with_nothing_to_fetch_says_what_it_changed_then_that_nothing_did() {
    let b = Binary::new();
    git(b.dir.path(), &["init", "-q"]);
    b.run(&["init", "--here"]);

    let check = b.run(&["sync", "--check"]);
    assert_eq!((check.code, check.stdout.as_str()), (3, ""));

    let first = b.run(&["sync"]);
    assert_eq!((first.code, first.stdout.as_str()), (0, ""));
    assert_eq!(first.stderr, "updated refs.lock\nupdated AGENTS.md\n");

    let second = b.run(&["sync"]);
    assert_eq!((second.code, second.stdout.as_str()), (0, ""));
    assert_eq!(second.stderr, "nothing changed\n");

    let quiet = b.run(&["sync", "-q"]);
    assert_eq!(
        (quiet.code, quiet.stdout, quiet.stderr),
        (0, "".into(), "".into())
    );
    assert_eq!(b.run(&["sync", "--check"]).stderr, "up to date\n");
}

#[test]
fn the_binary_edits_without_sync_say_what_changed_and_hint() {
    let b = Binary::new();
    fs::write(b.dir.path().join("refs.toml"), AB).unwrap();

    let added = b.run(&["add", "https://github.com/o/c", "--no-sync"]);
    assert_eq!((added.code, added.stdout.as_str()), (0, ""));
    assert_eq!(
        added.stderr,
        "updated refs.toml\nrun `refs sync` to bring the project up to date\n"
    );

    let quiet = b.run(&["remove", "c", "--no-sync", "-q"]);
    assert_eq!(
        (quiet.code, quiet.stdout, quiet.stderr),
        (0, "".into(), "".into())
    );

    let nothing = b.run(&["enable", "a", "--no-sync"]);
    assert_eq!((nothing.code, nothing.stdout.as_str()), (0, ""));
    assert_eq!(nothing.stderr, "nothing changed\n");

    let refused = b.run(&["remove", "nope", "--no-sync", "-q"]);
    assert_eq!((refused.code, refused.stdout.as_str()), (1, ""));
    assert!(refused.stderr.contains("nope"), "{}", refused.stderr);
}
