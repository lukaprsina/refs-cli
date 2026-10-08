//! `add` learns `packages` from the new Checkout (ADR 0009) through `cli::run_on`, with the
//! answers scripted and a fake `Source`. The fake makes no files, so each test writes the
//! Manifests where the Checkout would be.

use std::collections::VecDeque;
use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Terminal, run_on};
use refs_cli::prompt::{Abort, Prompter};
use refs_cli::source::fake::{Call, FakeSource};
use refs_cli::update::Unmanaged;
use tempfile::TempDir;

/// The questions asked with the default each had, and the answers to give them in order. An
/// empty text answer takes the default.
struct Script {
    answers: VecDeque<Answer>,
    asked: Vec<(String, Option<String>)>,
}

enum Answer {
    Text(&'static str),
    Yes(bool),
    Cancel,
    Fail(&'static str),
}

impl Script {
    fn new(answers: impl IntoIterator<Item = Answer>) -> Script {
        Script {
            answers: answers.into_iter().collect(),
            asked: Vec::new(),
        }
    }

    fn next(&mut self, message: &str, default: Option<&str>) -> Answer {
        self.asked
            .push((message.to_owned(), default.map(str::to_owned)));
        self.answers
            .pop_front()
            .unwrap_or_else(|| panic!("no answer left for {message:?}"))
    }
}

impl Prompter for Script {
    fn text(
        &mut self,
        message: &str,
        default: Option<&str>,
        _validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        match self.next(message, default) {
            Answer::Text("") => Ok(default.unwrap_or("").to_owned()),
            Answer::Text(text) => Ok(text.to_owned()),
            Answer::Cancel => Err(Abort::Cancelled),
            Answer::Fail(why) => Err(Abort::Failed(why.to_owned())),
            Answer::Yes(_) => panic!("{message:?} is a text question"),
        }
    }

    fn suggest(
        &mut self,
        message: &str,
        _suggestions: &[String],
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        self.text(message, None, validate)
    }

    fn confirm(&mut self, message: &str) -> Result<bool, Abort> {
        match self.next(message, None) {
            Answer::Yes(yes) => Ok(yes),
            Answer::Cancel => Err(Abort::Cancelled),
            Answer::Text(_) | Answer::Fail(_) => panic!("{message:?} is a confirm question"),
        }
    }
}

struct Run {
    code: u8,
    err: String,
}

/// A project with no repos, and a Manifest in the place where the Checkout of `foo` will be.
fn project_with_checkout_of_foo(manifest: &str, text: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("refs.toml"), "").unwrap();
    let checkout = dir.path().join(".references/foo");
    fs::create_dir_all(&checkout).unwrap();
    fs::write(checkout.join(manifest), text).unwrap();
    dir
}

fn config_text(dir: &Path) -> String {
    fs::read_to_string(dir.join("refs.toml")).unwrap()
}

/// `refs add https://github.com/o/foo --id foo --ref main --description d --paths src --start README.md`
/// plus `args`, so the only questions left are the group and `packages`.
fn add_foo(
    dir: &Path,
    source: &FakeSource,
    args: &[&str],
    prompter: Option<&mut dyn Prompter>,
) -> Run {
    let base = [
        "refs",
        "add",
        "https://github.com/o/foo",
        "--id",
        "foo",
        "--ref",
        "main",
        "--description",
        "d",
        "--paths",
        "src",
        "--start",
        "README.md",
    ];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_on(
        base.iter().chain(args).map(Into::into),
        dir,
        |_, _| Ok(Box::new(source)),
        &mut out,
        &mut err,
        Terminal::default(),
        Outside {
            prompter,
            updater: &Unmanaged,
        },
    );
    Run {
        code,
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn the_confirm_defaults_to_the_inferred_names_and_the_answer_is_written() {
    let dir = project_with_checkout_of_foo("package.json", r#"{ "name": "foo-js" }"#);
    let source = FakeSource::new();
    let mut script = Script::new([Answer::Yes(false), Answer::Text("")]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        script.asked.last().unwrap(),
        &(
            "Packages (space-separated)".to_owned(),
            Some("foo-js".to_owned())
        )
    );
    let config = config_text(dir.path());
    assert!(config.contains("packages = [\"foo-js\"]"), "{config}");
}

#[test]
fn a_cancel_at_the_confirm_leaves_the_repo_added_without_packages_and_says_so() {
    let dir = project_with_checkout_of_foo("package.json", r#"{ "name": "foo-js" }"#);
    let source = FakeSource::new();
    let mut script = Script::new([Answer::Yes(false), Answer::Cancel]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(config.contains("[repos.foo]"), "{config}");
    assert!(!config.contains("packages"), "{config}");
    assert!(
        run.err.contains("added `foo` without packages"),
        "{}",
        run.err
    );
}

#[test]
fn without_a_terminal_the_inferred_names_are_written() {
    let dir = project_with_checkout_of_foo("Cargo.toml", "[package]\nname = \"foo-bar\"\n");
    let source = FakeSource::new();

    let run = add_foo(dir.path(), &source, &[], None);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(dir.path()).contains("packages = [\"foo-bar\"]"));
}

#[test]
fn a_miss_asks_with_no_default_and_an_empty_answer_writes_nothing() {
    let dir = project_with_checkout_of_foo("README.md", "# foo");
    let source = FakeSource::new();
    let mut script = Script::new([Answer::Yes(false), Answer::Text("")]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.asked.last().unwrap().1, None);
    assert!(!config_text(dir.path()).contains("packages"));
    assert!(
        run.err.contains("added `foo` without packages"),
        "{}",
        run.err
    );
}

#[test]
fn a_failing_terminal_is_reported_as_such_and_the_repo_stays_added() {
    let dir = project_with_checkout_of_foo("package.json", r#"{ "name": "foo-js" }"#);
    let source = FakeSource::new();
    let mut script = Script::new([Answer::Yes(false), Answer::Fail("no tty")]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("cannot prompt: no tty"), "{}", run.err);
    assert!(
        run.err.contains("added `foo` without packages"),
        "{}",
        run.err
    );
    assert!(!config_text(dir.path()).contains("packages"));
}

#[test]
fn explicit_packages_and_no_sync_infer_nothing() {
    let dir = project_with_checkout_of_foo("package.json", r#"{ "name": "foo-js" }"#);
    let source = FakeSource::new();
    let mut script = Script::new([Answer::Yes(false)]);
    let run = add_foo(
        dir.path(),
        &source,
        &["--packages", "mine"],
        Some(&mut script),
    );
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(dir.path()).contains("packages = [\"mine\"]"));
    assert_eq!(script.asked.len(), 1, "{:?}", script.asked);

    let dir = project_with_checkout_of_foo("package.json", r#"{ "name": "foo-js" }"#);
    let source = FakeSource::new();
    let run = add_foo(dir.path(), &source, &["--no-sync"], None);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(!config_text(dir.path()).contains("packages"));
    assert!(source.calls().is_empty());
}

#[test]
fn the_second_sync_does_not_resolve_fetch_or_check_out_again() {
    let dir = project_with_checkout_of_foo("package.json", r#"{ "name": "foo-js" }"#);
    let source = FakeSource::new();

    let run = add_foo(dir.path(), &source, &[], None);

    assert_eq!(run.code, 0, "{}", run.err);
    let calls = source.calls();
    let count = |wanted: fn(&&Call) -> bool| calls.iter().filter(wanted).count();
    assert_eq!(count(|c| matches!(c, Call::Resolve(_))), 1, "{calls:?}");
    assert_eq!(
        count(|c| matches!(c, Call::Materialise { .. })),
        1,
        "{calls:?}"
    );
    assert_eq!(
        calls.last(),
        Some(&Call::Verify {
            id: "foo".into(),
            offline: true
        })
    );
    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(agents.contains("foo-js"), "{agents}");
}
