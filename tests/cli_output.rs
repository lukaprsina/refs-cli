//! What each command says, and where: status lines on stderr, data on stdout, `-q`, hints
//! and exit codes. `cli::run_with` takes the two streams, so the in-process tests (against
//! the fake `Source`) assert stdout and stderr separately; the binary tests below do the
//! same through a real process for what needs no `Source`.

mod common;

use std::fs;

use common::git;

use refs_cli::cli::run_with;
use refs_cli::source::Observed;
use refs_cli::source::fake::FakeSource;
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
