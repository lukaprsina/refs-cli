//! `add` shows one summary and asks `Customize?` (ADR 0011) through `cli::run_on`, with the
//! answers scripted and a fake `Source` that serves the repo.

use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Terminal, run_on};
use refs_cli::prompt::Prompter;
use refs_cli::self_update::Unmanaged;
use refs_cli::source::fake::FakeSource;
use tempfile::TempDir;

mod common;
use common::script::{Answer, Script};

const URL: &str = "https://github.com/o/foo";
const CUSTOMIZE: &str = "Customize?";

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

fn serving(files: &[(&str, &str)]) -> FakeSource {
    let source = FakeSource::new();
    source.set_tree(URL, &"b".repeat(40), files);
    source
}

/// A monorepo: a root package with a description, two packages under `packages/` and one
/// under `other/`, and docs in both places.
fn monorepo() -> FakeSource {
    serving(&[
        (
            "package.json",
            r#"{ "name": "root-js", "description": "The root" }"#,
        ),
        ("packages/web/package.json", r#"{ "name": "web-js" }"#),
        ("other/package.json", r#"{ "name": "other-js" }"#),
        ("packages/web/GUIDE.md", ""),
        ("other/NOTES.md", ""),
    ])
}

fn add_foo(
    dir: &Path,
    source: &FakeSource,
    args: &[&str],
    prompter: Option<&mut dyn Prompter>,
) -> Run {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_on(
        ["refs", "add", URL, "--ref", "main"]
            .iter()
            .chain(args)
            .map(Into::into),
        dir,
        |_, _| Ok(Box::new(source)),
        &mut out,
        &mut err,
        Terminal::default(),
        Outside {
            prompter,
            updater: &Unmanaged,
            registry: &refs_cli::registry::Unavailable,
        },
    );
    Run {
        code,
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn enter_on_customize_writes_what_the_summary_showed() {
    let dir = project();
    let mut script = Script::new([Answer::Yes(false)]);

    let run = add_foo(dir.path(), &monorepo(), &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.questions(), [CUSTOMIZE]);
    for shown in [
        "foo",
        "main (bbbbbbb)",
        "(everything)",
        "other-js, root-js, web-js",
        "The root",
        "(none)",
    ] {
        assert!(run.err.contains(shown), "{shown:?} in {}", run.err);
    }
    let config = config_text(dir.path());
    assert!(config.contains(r#"description = "The root""#), "{config}");
    assert!(
        config.contains(r#"packages = ["other-js", "root-js", "web-js"]"#),
        "{config}"
    );
}

#[test]
fn yes_and_then_enter_for_every_question_writes_what_no_writes() {
    let no = project();
    add_foo(
        no.path(),
        &monorepo(),
        &[],
        Some(&mut Script::takes_defaults(false)),
    );
    let yes = project();
    let mut script = Script::takes_defaults(true);

    let run = add_foo(yes.path(), &monorepo(), &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        script.questions()[..7],
        [
            CUSTOMIZE,
            "Id",
            "Group (none yet; Enter for none, or a name to create one)",
            "Description (one line on what the repo is)",
            "Paths to check out (type to filter; none selected: the whole repo)",
            "Files to read first (type to filter; none selected: none)",
            "Packages (type to filter; none selected: none)",
        ]
    );
    assert_eq!(config_text(yes.path()), config_text(no.path()));
}

#[test]
fn narrowing_paths_narrows_the_packages_and_the_start_candidates() {
    let dir = project();
    let mut script = Script::new([
        Answer::Yes(true),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Pick(&["packages"]),
        Answer::Pick(&["packages/web/GUIDE.md"]),
        Answer::Pick(&["root-js", "web-js"]),
    ]);

    let run = add_foo(dir.path(), &monorepo(), &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.offered[1], ["packages/web/GUIDE.md"]);
    assert_eq!(script.preselected[2], ["root-js", "web-js"]);
    let config = config_text(dir.path());
    assert!(
        config.contains(r#"packages = ["root-js", "web-js"]"#),
        "{config}"
    );
}

#[test]
fn narrowing_paths_says_how_many_packages_were_left_out_even_when_quiet() {
    let dir = project();
    let mut script = Script::new([
        Answer::Yes(true),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Pick(&["packages"]),
        Answer::Pick(&[]),
        Answer::Pick(&["root-js", "web-js"]),
    ]);

    let run = add_foo(dir.path(), &monorepo(), &["-q"], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains(
            "packages: left out 1 of 3 that are outside the chosen paths; name others with `--packages`"
        ),
        "{}",
        run.err
    );
}

#[test]
fn choosing_paths_that_hold_every_package_says_nothing_about_packages() {
    let dir = project();
    let mut script = Script::takes_defaults(true);

    let run = add_foo(dir.path(), &monorepo(), &[], Some(&mut script));

    assert!(!run.err.contains("left out"), "{}", run.err);
}

#[test]
fn a_cleared_description_and_packages_are_written_as_none() {
    let dir = project();
    let mut script = Script::new([
        Answer::Yes(true),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Clear,
        Answer::Pick(&[]),
        Answer::Pick(&[]),
        Answer::Pick(&[]),
    ]);

    let run = add_foo(dir.path(), &monorepo(), &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(
        !config.contains("description") && !config.contains("packages"),
        "{config}"
    );
}

#[test]
fn a_description_nobody_found_is_asked_before_the_summary_and_not_again() {
    let dir = project();
    let source = serving(&[("README.md", ""), ("docs/a.md", "")]);
    let mut script = Script::new([
        Answer::Text("A library"),
        Answer::Yes(true),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Pick(&[]),
        Answer::Pick(&[]),
        Answer::Text(""),
    ]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.asked_about("Description").len(), 1);
    assert!(script.questions()[0].starts_with("Description"));
    assert!(config_text(dir.path()).contains(r#"description = "A library""#));
}

#[test]
fn a_description_that_was_found_is_not_asked_before_the_summary() {
    let dir = project();
    let mut script = Script::new([Answer::Yes(false)]);

    add_foo(dir.path(), &monorepo(), &[], Some(&mut script));

    assert!(script.asked_about("Description").is_empty());
}

#[test]
fn a_taken_default_id_is_asked_first_and_a_taken_answer_is_refused() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("refs.toml"),
        "[repos.foo]\nurl = \"https://github.com/x/foo\"\n",
    )
    .unwrap();
    let mut script = Script::new([
        Answer::Text("foo"),
        Answer::Text("foo-two"),
        Answer::Yes(false),
    ]);

    let run = add_foo(dir.path(), &monorepo(), &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.questions()[0], "Id");
    assert!(script.rejected[0].contains("already a repo"));
    assert!(config_text(dir.path()).contains("[repos.foo-two]"));
}

#[test]
fn a_taken_id_from_a_flag_is_an_error() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("refs.toml"),
        "[repos.foo]\nurl = \"https://github.com/x/foo\"\n",
    )
    .unwrap();

    let run = add_foo(dir.path(), &monorepo(), &["--id", "foo"], None);

    assert_ne!(run.code, 0);
}

#[test]
fn no_input_prints_no_summary_and_asks_nothing() {
    let dir = project();
    let mut script = Script::new([]);

    let run = add_foo(dir.path(), &monorepo(), &["--no-input"], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(script.asked.is_empty());
    assert!(!run.err.contains("(everything)"), "{}", run.err);
}

#[test]
fn the_summary_is_shown_even_when_quiet() {
    let dir = project();
    let mut script = Script::new([Answer::Yes(false)]);

    let run = add_foo(dir.path(), &monorepo(), &["-q"], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("(everything)"), "{}", run.err);
    assert!(!run.err.contains("equivalent:"), "{}", run.err);
}

#[test]
fn cancelling_at_customize_edits_nothing() {
    let dir = project();
    let mut script = Script::new([Answer::Cancel]);

    let run = add_foo(dir.path(), &monorepo(), &[], Some(&mut script));

    assert_eq!(run.code, 1);
    assert_eq!(config_text(dir.path()), "");
}
