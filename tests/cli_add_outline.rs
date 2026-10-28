//! `add` looks at the repo before it asks (ADR 0011) through `cli::run_on`, with the answers
//! scripted and a fake `Source` and `Registry`. What is observed is whether the outline ran
//! (the fake's calls), when a bad URL or ref fails, and that `refs.toml` stays as it was.

use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Styled, run_on};
use refs_cli::registry::fake::FakeRegistry;
use refs_cli::registry::{Ecosystem, Found};
use refs_cli::self_update::fake::Unmanaged;
use refs_cli::source::fake::{Call, FakeSource, Method};
use refs_cli::ui::Prompter;
use tempfile::TempDir;

mod common;
use common::script::{Answer, Script};

const URL: &str = "https://github.com/o/foo";

struct Run {
    code: u8,
    err: String,
}

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("refs.toml"), "").unwrap();
    dir
}

fn config_text(dir: &Path) -> String {
    fs::read_to_string(dir.join("refs.toml")).unwrap()
}

/// `refs add <args>`, asking through `prompter` if there is one.
fn add(dir: &Path, source: &FakeSource, args: &[&str], prompter: Option<&mut dyn Prompter>) -> Run {
    // Leaked: the prompter's lifetime outlives anything this function owns.
    let registry: &FakeRegistry = Box::leak(Box::new(FakeRegistry::new().with(
        Ecosystem::Npm,
        "foo",
        Found {
            url: URL.into(),
            directory: None,
            description: None,
        },
    )));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_on(
        ["refs", "add"]
            .into_iter()
            .chain(args.iter().copied())
            .map(Into::into),
        dir,
        |_, _| Ok(Box::new(source.clone())),
        &mut out,
        &mut err,
        Styled::default(),
        Outside {
            prompter,
            updater: &Unmanaged,
            registry,
        },
    );
    Run {
        code,
        err: String::from_utf8(err).unwrap(),
    }
}

fn outlined(source: &FakeSource) -> bool {
    source.calls().contains(&Call::Outline(URL.into()))
}

#[test]
fn a_plain_url_add_with_a_terminal_outlines_the_repo() {
    let dir = project();
    let source = FakeSource::new();
    let mut script = Script::takes_defaults(false);

    let run = add(
        dir.path(),
        &source,
        &[URL, "--id", "foo"],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(outlined(&source));
}

#[test]
fn a_forge_shorthand_is_outlined_like_a_url() {
    let dir = project();
    let source = FakeSource::new();

    let run = add(dir.path(), &source, &["gh:o/foo"], None);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(outlined(&source));
}

#[test]
fn a_no_input_add_without_packages_outlines_the_repo() {
    let dir = project();
    let source = FakeSource::new();

    let run = add(dir.path(), &source, &[URL, "--no-input"], None);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(outlined(&source));
}

#[test]
fn packages_given_with_no_input_skip_the_outline() {
    let dir = project();
    let source = FakeSource::new();

    let run = add(dir.path(), &source, &[URL, "--packages", "foo"], None);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(!outlined(&source));
}

#[test]
fn packages_given_with_a_terminal_still_outline_the_repo() {
    let dir = project();
    let source = FakeSource::new();
    let mut script = Script::takes_defaults(false);

    let args = [URL, "--id", "foo", "--packages", "foo"];
    let run = add(dir.path(), &source, &args, Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(outlined(&source));
}

#[test]
fn offline_and_no_sync_do_not_outline() {
    for args in [
        &[URL, "--offline"][..],
        &[URL, "--no-sync"],
        &["npm:foo", "--no-sync"],
    ] {
        for terminal in [true, false] {
            let dir = project();
            let source = FakeSource::new();
            let mut script = Script::takes_defaults(false);
            let prompter = terminal.then_some(&mut script as &mut dyn Prompter);

            add(dir.path(), &source, args, prompter);

            assert!(!outlined(&source), "{args:?}, terminal: {terminal}");
        }
    }
}

#[test]
fn the_ref_is_asked_first_and_a_bad_ref_fails_before_any_other_question() {
    let dir = project();
    let source = FakeSource::new();
    source.fail(URL, Method::Outline, "no such ref: nope");
    let mut script = Script::new([Answer::Text("nope")]);

    let run = add(dir.path(), &source, &[URL], Some(&mut script));

    assert_ne!(run.code, 0);
    assert!(run.err.contains("no such ref: nope"), "{}", run.err);
    assert_eq!(script.questions().len(), 1, "{:?}", script.questions());
    assert!(script.questions()[0].starts_with("Ref"));
    assert_eq!(config_text(dir.path()), "");
}

#[test]
fn a_ref_given_by_flag_is_not_asked_and_a_bad_url_fails_before_any_question() {
    let dir = project();
    let source = FakeSource::new();
    source.fail(URL, Method::Outline, "repository not found");
    let mut script = Script::new([]);

    let run = add(
        dir.path(),
        &source,
        &[URL, "--ref", "v1"],
        Some(&mut script),
    );

    assert_ne!(run.code, 0);
    assert!(run.err.contains("repository not found"), "{}", run.err);
    assert!(script.questions().is_empty());
    assert_eq!(config_text(dir.path()), "");
}

#[test]
fn a_cancel_after_the_outline_edits_nothing() {
    let dir = project();
    let source = FakeSource::new();
    let mut script = Script::new([Answer::Text("main"), Answer::Cancel]);

    let run = add(dir.path(), &source, &[URL], Some(&mut script));

    assert_ne!(run.code, 0);
    assert!(outlined(&source));
    assert_eq!(config_text(dir.path()), "");
}
