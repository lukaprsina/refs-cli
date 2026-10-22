//! `add` offers the repo's directories for `paths` and its docs files for `start` (ADR 0011)
//! through `cli::run_on`, with the answers scripted and a fake `Source` that serves the tree.

use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Terminal, run_on};
use refs_cli::prompt::Prompter;
use refs_cli::source::fake::FakeSource;
use refs_cli::update::Unmanaged;
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

fn serving(files: &[&str]) -> FakeSource {
    let source = FakeSource::new();
    let files: Vec<(&str, &str)> = files.iter().map(|path| (*path, "")).collect();
    source.set_tree(URL, &"b".repeat(40), &files);
    source
}

/// `refs add https://github.com/o/foo --ref main` plus `args`.
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

/// The answers up to and including "Customize paths and start?" (yes): id, group and
/// description left as offered, then the pick answers.
fn customizing(picks: impl IntoIterator<Item = Answer>) -> Script {
    let first = [
        Answer::Text(""),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Yes(true),
    ];
    Script::new(first.into_iter().chain(picks))
}

#[test]
fn the_directories_of_the_repo_are_offered_and_the_picked_ones_written() {
    let dir = project();
    let source = serving(&["README.md", "docs/a.md", "src/lib.rs", "src/util/x.rs"]);
    let mut script = customizing([Answer::Pick(&["docs", "src/util"]), Answer::Pick(&[])]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.offered[0], ["docs", "src", "src/util"]);
    let config = config_text(dir.path());
    assert!(
        config.contains(r#"paths = ["docs", "src/util"]"#),
        "{config}"
    );
}

#[test]
fn selecting_nothing_writes_neither_paths_nor_start() {
    let dir = project();
    let source = serving(&["README.md", "docs/a.md"]);
    let mut script = customizing([Answer::Pick(&[]), Answer::Pick(&[])]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(
        !config.contains("paths") && !config.contains("start"),
        "{config}"
    );
}

#[test]
fn a_directory_and_its_child_write_only_the_directory() {
    let dir = project();
    let source = serving(&["packages/solid-js/src/a.ts", "packages/web/b.ts"]);
    let mut script = customizing([
        Answer::Pick(&["packages", "packages/solid-js"]),
        Answer::Pick(&[]),
    ]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(config.contains(r#"paths = ["packages"]"#), "{config}");
}

#[test]
fn start_offers_only_docs_files_the_checkout_will_hold() {
    let dir = project();
    let source = serving(&[
        "README.md",
        "LICENSE",
        "notes.txt",
        "guide/intro.MDX",
        "guide/ref.rst",
        "guide/code.rs",
        "packages/README",
        "packages/web/docs/a.md",
        "packages/other/docs/b.md",
        "elsewhere/c.md",
    ]);
    let mut script = customizing([
        Answer::Pick(&["packages/web"]),
        Answer::Pick(&["packages/web/docs/a.md"]),
    ]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        script.offered[1],
        ["README.md", "packages/README", "packages/web/docs/a.md"]
    );
    let config = config_text(dir.path());
    assert!(
        config.contains(r#"start = ["packages/web/docs/a.md"]"#),
        "{config}"
    );
}

#[test]
fn a_repo_without_directories_is_not_asked_for_paths() {
    let dir = project();
    let source = serving(&["README.md", "lib.rs"]);
    let mut script = customizing([Answer::Pick(&["README.md"])]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.offered, [["README.md"]]);
}

#[test]
fn directories_past_the_cap_are_counted_in_a_status_line() {
    let dir = project();
    let files: Vec<String> = (0..5003).map(|i| format!("d{i}/f.rs")).collect();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    let source = serving(&files);
    let mut script = customizing([Answer::Pick(&[]), Answer::Pick(&[])]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.offered[0].len(), 5000);
    assert!(
        run.err.contains("listed 5000 of 5003 directories") && run.err.contains("`--paths`"),
        "{}",
        run.err
    );
}
