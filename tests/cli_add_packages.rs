//! `add` learns `packages` and a description from the repo's Manifests before it edits anything
//! (ADR 0009, 0011) through `cli::run_on`, with the answers scripted and a fake `Source` that
//! serves the Manifests.

use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Terminal, run_on};
use refs_cli::prompt::Prompter;
use refs_cli::source::fake::{Call, FakeSource, Method};
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

/// A fake that serves `files` for the repo.
fn serving(files: &[(&str, &str)]) -> FakeSource {
    let source = FakeSource::new();
    source.set_tree(URL, &"b".repeat(40), files);
    source
}

fn solid() -> FakeSource {
    serving(&[
        (
            "package.json",
            r#"{ "name": "solid-js", "description": "A UI\nlibrary" }"#,
        ),
        ("solid-js/h/package.json", r#"{ "name": "solid-js/h" }"#),
        ("solid-js/web/package.json", r#"{ "name": "solid-js/web" }"#),
        ("packages/web/package.json", r#"{ "name": "@solidjs/web" }"#),
        (
            "packages/store/package.json",
            r#"{ "name": "@solidjs/store" }"#,
        ),
        (
            "packages/docs/package.json",
            r#"{ "name": "docs", "private": true }"#,
        ),
        ("other/package.json", r#"{ "name": "other-js" }"#),
        ("README.md", "# solid"),
    ])
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

#[test]
fn a_plain_url_add_writes_the_packages_in_one_edit_and_one_sync() {
    let dir = project();
    let source = solid();
    let mut script = Script::takes_defaults(false);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(
        config.contains(r#"packages = ["@solidjs/store", "@solidjs/web", "other-js", "solid-js"]"#),
        "{config}"
    );
    assert_eq!(
        run.err.matches("updated refs.toml").count(),
        1,
        "{}",
        run.err
    );
    assert_eq!(run.err.matches("AGENTS.md").count(), 1, "{}", run.err);
    let calls = source.calls();
    let resolves = calls.iter().filter(|c| matches!(c, Call::Resolve(_)));
    assert_eq!(resolves.count(), 1, "{calls:?}");
}

#[test]
fn only_manifests_the_checkout_will_hold_count() {
    let dir = project();
    let source = solid();

    let run = add_foo(dir.path(), &source, &["--paths", "packages/web"], None);

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(
        config.contains(r#"packages = ["@solidjs/web", "solid-js"]"#),
        "{config}"
    );
}

#[test]
fn manifests_beyond_the_cap_are_counted_in_a_status_line() {
    let dir = project();
    let names: Vec<(String, String)> = (0..230)
        .map(|i| {
            (
                format!("p{i}/package.json"),
                format!(r#"{{ "name": "n{i}" }}"#),
            )
        })
        .collect();
    let files: Vec<(&str, &str)> = names.iter().map(|(p, t)| (&**p, &**t)).collect();
    let source = serving(&files);

    let run = add_foo(dir.path(), &source, &[], None);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("left 30 unread"), "{}", run.err);
}

#[test]
fn the_description_starts_from_the_root_manifest_and_a_flag_wins() {
    let dir = project();
    let source = solid();
    let mut script = Script::takes_defaults(false);
    add_foo(dir.path(), &source, &[], Some(&mut script));
    let asked = script.asked_about("Description");
    assert_eq!(asked[0].1.as_deref(), Some("A UI library"));
    assert!(config_text(dir.path()).contains("description = \"A UI library\""));

    let dir = project();
    let mut script = Script::takes_defaults(false);
    add_foo(
        dir.path(),
        &source,
        &["--description", "mine"],
        Some(&mut script),
    );
    assert!(script.asked_about("Description").is_empty());
    assert!(config_text(dir.path()).contains("description = \"mine\""));
}

#[test]
fn an_empty_description_answer_clears_the_default() {
    let dir = project();
    let source = solid();
    let mut script = Script::new([
        Answer::Text(""),
        Answer::Text(""),
        Answer::Clear,
        Answer::Yes(false),
    ]);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(!config_text(dir.path()).contains("description"));
}

#[test]
fn no_input_writes_the_inferred_packages_but_no_description() {
    let dir = project();
    let source = solid();

    let run = add_foo(dir.path(), &source, &[], None);

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(config.contains("packages = ["), "{config}");
    assert!(!config.contains("description"), "{config}");
}

#[test]
fn no_sync_infers_nothing_and_still_adds() {
    let dir = project();
    let source = solid();

    let run = add_foo(dir.path(), &source, &["--no-sync"], None);

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(
        config.contains("[repos.foo]") && !config.contains("packages"),
        "{config}"
    );
    assert!(source.calls().is_empty());
}

#[test]
fn offline_does_not_look_at_the_repo() {
    let dir = project();
    let source = solid();
    // The sync itself needs a Lock offline, so only what was looked at is checked.
    add_foo(dir.path(), &source, &["--offline"], None);
    assert!(!config_text(dir.path()).contains("packages"));
    assert!(!source.calls().contains(&Call::Outline(URL.into())));
}

#[test]
fn the_equivalent_command_comes_before_the_sync_and_survives_its_failure() {
    let dir = project();
    let source = solid();
    source.fail("foo", Method::Materialise, "boom");
    let mut script = Script::takes_defaults(false);

    let run = add_foo(dir.path(), &source, &[], Some(&mut script));

    assert_ne!(run.code, 0);
    let equivalent = run.err.find("equivalent: refs add").expect(&run.err);
    assert!(run.err[equivalent..].contains("--packages"), "{}", run.err);
    assert!(
        equivalent < run.err.find("boom").expect(&run.err),
        "{}",
        run.err
    );
}
