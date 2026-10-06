//! Interactive `add` (ADR 0008) through `cli::run_on`, with the answers scripted. Config-only
//! (`--no-sync`), so the `Source` factory panics if a command reaches for one.

use std::collections::VecDeque;
use std::fs;
use std::path::Path;

use refs_cli::cli::{Terminal, run_on};
use refs_cli::diagnostic::SourceError;
use refs_cli::prompt::{Abort, Prompter};
use refs_cli::source::Source;
use tempfile::TempDir;

const CONFIG: &str = r#"[groups.core]
description = "The core"

[repos.solid]
url = "https://github.com/solidjs/solid"
"#;

/// The questions asked, and the answers to give them in order. A text answer that fails
/// validation is recorded as a rejection and the next answer is tried, as a terminal would
/// re-ask. An empty text answer takes the default.
struct Script {
    answers: VecDeque<Answer>,
    asked: Vec<String>,
    rejected: Vec<String>,
}

enum Answer {
    Text(&'static str),
    Pick(usize),
    Yes(bool),
    Cancel,
}

impl Script {
    fn new(answers: impl IntoIterator<Item = Answer>) -> Script {
        Script {
            answers: answers.into_iter().collect(),
            asked: Vec::new(),
            rejected: Vec::new(),
        }
    }

    fn next(&mut self, message: &str) -> Answer {
        self.asked.push(message.to_owned());
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
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        loop {
            let answer = match self.next(message) {
                Answer::Text("") => default.unwrap_or("").to_owned(),
                Answer::Text(text) => text.to_owned(),
                Answer::Cancel => return Err(Abort::Cancelled),
                _ => panic!("{message:?} is a text question"),
            };
            match validate(&answer) {
                Ok(()) => return Ok(answer),
                Err(why) => self.rejected.push(why),
            }
        }
    }

    fn select(&mut self, message: &str, _options: &[String]) -> Result<usize, Abort> {
        match self.next(message) {
            Answer::Pick(index) => Ok(index),
            Answer::Cancel => Err(Abort::Cancelled),
            _ => panic!("{message:?} is a select question"),
        }
    }

    fn confirm(&mut self, message: &str) -> Result<bool, Abort> {
        match self.next(message) {
            Answer::Yes(yes) => Ok(yes),
            Answer::Cancel => Err(Abort::Cancelled),
            _ => panic!("{message:?} is a confirm question"),
        }
    }
}

struct Run {
    code: u8,
    err: String,
}

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("refs.toml"), CONFIG).unwrap();
    dir
}

fn config_text(dir: &Path) -> String {
    fs::read_to_string(dir.join("refs.toml")).unwrap()
}

fn refs(dir: &Path, args: &[&str], prompter: Option<&mut dyn Prompter>) -> Run {
    let args = std::iter::once("refs").chain(args.iter().copied());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_on(
        args.map(Into::into),
        dir,
        |_, _| -> Result<Box<dyn Source>, SourceError> {
            panic!("a config-only command asked for a Source")
        },
        &mut out,
        &mut err,
        Terminal::default(),
        prompter,
    );
    Run {
        code,
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn a_bare_add_asks_for_everything_and_writes_the_answers() {
    let dir = project();
    let mut script = Script::new([
        Answer::Text("https://github.com/solidjs/solid-router"),
        Answer::Text(""),
        Answer::Pick(1),
        Answer::Text("next"),
        Answer::Text("The router"),
        Answer::Yes(true),
        Answer::Text("src docs"),
        Answer::Text("@solidjs/router"),
        Answer::Text(""),
    ]);

    let run = refs(dir.path(), &["add", "--no-sync"], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(
        config.contains(
            r#"[repos.solid-router]
url = "https://github.com/solidjs/solid-router"
group = "core"
ref = "next"
description = "The router"
paths = ["src", "docs"]
packages = ["@solidjs/router"]
"#
        ),
        "{config}"
    );
    assert!(
        run.err.contains(
            "refs add https://github.com/solidjs/solid-router --group core --ref next \
             --description \"The router\" --paths src docs --packages @solidjs/router"
        ),
        "{}",
        run.err
    );
}

#[test]
fn a_value_given_as_a_flag_is_not_asked_again() {
    let dir = project();
    let mut script = Script::new([Answer::Text(""), Answer::Pick(0), Answer::Yes(false)]);

    let run = refs(
        dir.path(),
        &[
            "add",
            "https://github.com/o/lib",
            "--ref",
            "v2",
            "--description",
            "A lib",
            "--no-sync",
        ],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        script.asked,
        ["Id", "Group", "Customize paths, packages and start?"]
    );
    assert!(config_text(dir.path()).contains("ref = \"v2\""));
}

#[test]
fn a_bad_answer_is_asked_again() {
    let dir = project();
    let mut script = Script::new([
        Answer::Text("solid"), // taken
        Answer::Text("Not Valid"),
        Answer::Text("router"),
        Answer::Pick(0),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Yes(false),
    ]);

    let run = refs(
        dir.path(),
        &["add", "https://github.com/o/lib", "--no-sync"],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.rejected.len(), 2, "{:?}", script.rejected);
    assert!(script.rejected[0].contains("already a repo"));
    assert!(config_text(dir.path()).contains("[repos.router]"));
}

#[test]
fn cancelling_changes_nothing() {
    let dir = project();
    let mut script = Script::new([Answer::Text(""), Answer::Cancel]);

    let run = refs(
        dir.path(),
        &["add", "https://github.com/o/lib", "--no-sync"],
        Some(&mut script),
    );

    assert_eq!(run.code, 1);
    assert!(run.err.contains("cancelled"), "{}", run.err);
    assert_eq!(config_text(dir.path()), CONFIG);
}

#[test]
fn no_input_asks_nothing() {
    let dir = project();
    let mut script = Script::new([]);

    let run = refs(
        dir.path(),
        &["add", "https://github.com/o/lib", "--no-input", "--no-sync"],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(script.asked.is_empty());
    assert!(config_text(dir.path()).contains("[repos.lib]"));
}

#[test]
fn without_a_terminal_nothing_is_asked_and_a_missing_url_is_a_usage_error() {
    let dir = project();

    let run = refs(dir.path(), &["add", "--no-sync"], None);

    assert_eq!(run.code, 2);
    assert!(run.err.contains("URL is required"), "{}", run.err);
    assert_eq!(config_text(dir.path()), CONFIG);

    let run = refs(
        dir.path(),
        &["add", "https://github.com/o/lib", "--no-sync"],
        None,
    );
    assert_eq!(run.code, 0, "{}", run.err);
}
