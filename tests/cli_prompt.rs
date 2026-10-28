//! Interactive `add` (ADR 0008) through `cli::run_on`, with the answers scripted. Config-only
//! (`--no-sync`), so the `Source` factory panics if a command reaches for one.

use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Styled, run_on};
use refs_cli::diagnostic::SourceError;
use refs_cli::source::Source;
use refs_cli::ui::Prompter;
use tempfile::TempDir;

mod common;
use common::script::{Answer, Script};

const CONFIG: &str = r#"[groups.core]
description = "The core"

[repos.solid]
url = "https://github.com/solidjs/solid"
"#;

const GROUP: &str = "Group (Enter for none; type to filter, or a new name to create one)";

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
        Styled::default(),
        Outside::offline(prompter),
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
        Answer::Text("next"),
        Answer::Text("The router"),
        Answer::Yes(true),
        Answer::Text(""),
        Answer::Text("core"),
        Answer::Text("src docs"),
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
"#
        ),
        "{config}"
    );
    assert!(
        run.err.contains(
            "refs add https://github.com/solidjs/solid-router --group core --ref next \
             --description \"The router\" --paths src docs"
        ),
        "{}",
        run.err
    );
}

#[test]
fn a_value_given_as_a_flag_is_not_asked_again() {
    let dir = project();
    let mut script = Script::takes_defaults(true);

    let run = refs(
        dir.path(),
        &[
            "add",
            "https://github.com/o/lib",
            "--ref",
            "v2",
            "--description",
            "A lib",
            "--packages",
            "lib",
            "--no-sync",
        ],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    for given in ["Ref", "Description", "Packages"] {
        assert!(script.asked_about(given).is_empty(), "{given}");
    }
    assert!(config_text(dir.path()).contains("ref = \"v2\""));
}

#[test]
fn any_one_field_left_open_is_enough_to_offer_to_customize() {
    let flags = [
        ("id", ["--id", "mine"]),
        ("group", ["--group", "core"]),
        ("paths", ["--paths", "src"]),
        ("start", ["--start", "README.md"]),
        ("packages", ["--packages", "lib"]),
    ];
    for (open, _) in flags {
        let dir = project();
        let mut script = Script::takes_defaults(false);
        // A missing description is asked before the summary, so it is never the one left open.
        let mut args = vec![
            "add",
            "https://github.com/o/lib",
            "--no-sync",
            "--description",
            "A lib",
        ];
        for (field, given) in &flags {
            if *field != open {
                args.extend(given);
            }
        }

        let run = refs(dir.path(), &args, Some(&mut script));

        assert_eq!(run.code, 0, "{open}: {}", run.err);
        assert_eq!(script.asked_about("Customize?").len(), 1, "{open}");
    }
}

#[test]
fn a_url_git_would_misread_is_asked_again_at_the_url_question() {
    let dir = project();
    let mut script = Script::new([
        Answer::Text("ext::sh -c true"),
        Answer::Text("-x"),
        Answer::Text("https://github.com/o/lib"),
        Answer::Text(""), // ref
        Answer::Text(""), // description
        Answer::Yes(false),
    ]);

    let run = refs(dir.path(), &["add", "--no-sync"], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.rejected.len(), 2, "{:?}", script.rejected);
    assert!(config_text(dir.path()).contains("url = \"https://github.com/o/lib\""));
}

#[test]
fn a_bad_answer_is_asked_again() {
    let dir = project();
    let mut script = Script::new([
        Answer::Text(""), // ref
        Answer::Text(""), // description
        Answer::Yes(true),
        Answer::Text("solid"), // taken
        Answer::Text("Not Valid"),
        Answer::Text("router"),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Text(""),
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
fn the_equivalent_command_is_printed_with_a_terminal_even_when_nothing_was_asked() {
    let dir = project();
    let mut script = Script::new([]);

    let run = refs(
        dir.path(),
        &[
            "add",
            "https://github.com/o/lib",
            "--no-sync",
            "--id",
            "lib",
            "--group",
            "core",
            "--ref",
            "main",
            "--description",
            "A lib",
            "--paths",
            "docs",
            "--start",
            "docs/a.md",
            "--packages",
            "lib",
        ],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(script.asked.is_empty());
    assert!(
        run.err
            .contains("equivalent: refs add https://github.com/o/lib"),
        "{}",
        run.err
    );
}

#[test]
fn no_input_prints_no_equivalent_command() {
    let dir = project();
    let mut script = Script::new([]);

    let run = refs(
        dir.path(),
        &["add", "https://github.com/o/lib", "--no-input", "--no-sync"],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(!run.err.contains("equivalent:"), "{}", run.err);
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

/// The answers for `add` with a ref, no sync and a description to ask for: the description, yes
/// at `Customize?`, id and the paths and start left as offered, and `group` for the
/// group.
fn group_answered(group: &'static str) -> Script {
    let (empty, yes) = (Answer::Text(""), Answer::Yes(true));
    let answers = [empty, yes, Answer::Text(""), Answer::Text(group)];
    let rest = [Answer::Text(""), Answer::Text("")];
    Script::new(answers.into_iter().chain(rest))
}

#[test]
fn the_first_group_can_be_created_by_typing_its_name() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("refs.toml"),
        "[repos.solid]
url = \"https://github.com/solidjs/solid\"
",
    )
    .unwrap();
    let mut script = group_answered("Solid core");

    let run = refs(
        dir.path(),
        &[
            "add",
            "https://github.com/o/lib",
            "--ref",
            "main",
            "--no-sync",
        ],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.suggested, [Vec::<String>::new()]);
    let config = config_text(dir.path());
    assert!(config.contains("[groups.\"Solid core\"]"), "{config}");
    assert!(config.contains("group = \"Solid core\""), "{config}");
}

#[test]
fn existing_groups_are_suggested_and_picking_one_creates_nothing() {
    let dir = project();
    let mut script = group_answered("core");

    let run = refs(
        dir.path(),
        &[
            "add",
            "https://github.com/o/lib",
            "--ref",
            "main",
            "--no-sync",
        ],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.suggested, [vec!["core".to_owned()]]);
    assert_eq!(config_text(dir.path()).matches("[groups.").count(), 1);
}

#[test]
fn enter_on_the_group_question_files_the_repo_under_no_group() {
    let dir = project();
    let mut script = group_answered("");

    let run = refs(
        dir.path(),
        &[
            "add",
            "https://github.com/o/lib",
            "--ref",
            "main",
            "--no-sync",
        ],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(script.rejected.is_empty(), "{:?}", script.rejected);
    assert!(script.questions().contains(&GROUP));
    assert!(!config_text(dir.path()).contains("group ="));
}

#[test]
fn with_no_groups_yet_the_question_says_a_name_creates_one() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("refs.toml"), "").unwrap();
    let mut script = Script::new([
        Answer::Text(""),
        Answer::Yes(true),
        Answer::Text(""),
        Answer::Cancel,
    ]);

    refs(
        dir.path(),
        &[
            "add",
            "https://github.com/o/lib",
            "--ref",
            "main",
            "--no-sync",
        ],
        Some(&mut script),
    );

    let group = script.questions()[3];
    assert!(group.contains("none yet"), "{group}");
}

#[test]
fn a_case_only_different_group_name_is_asked_again() {
    let dir = project();
    let mut script = Script::new([
        Answer::Text(""),
        Answer::Yes(true),
        Answer::Text(""),
        Answer::Text("Core"),
        Answer::Text("core"),
        Answer::Text(""),
        Answer::Text(""),
    ]);

    let run = refs(
        dir.path(),
        &[
            "add",
            "https://github.com/o/lib",
            "--ref",
            "main",
            "--no-sync",
        ],
        Some(&mut script),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.rejected.len(), 1, "{:?}", script.rejected);
    assert!(script.rejected[0].contains("did you mean `core`"));
}

#[test]
fn a_forge_shorthand_typed_at_the_url_question_is_expanded() {
    let dir = project();
    let mut script = Script::new([
        Answer::Text("gh:owner"),
        Answer::Text("gh:o/.github"),
        Answer::Text(""),
        Answer::Text(""),
        Answer::Yes(false),
    ]);

    let run = refs(dir.path(), &["add", "--no-sync"], Some(&mut script));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(script.rejected.len(), 1, "{:?}", script.rejected);
    let config = config_text(dir.path());
    assert!(
        config.contains("[repos.github]")
            && config.contains("url = \"https://github.com/o/.github\""),
        "{config}"
    );
    assert!(
        run.err
            .contains("equivalent: refs add https://github.com/o/.github"),
        "{}",
        run.err
    );
}

#[test]
fn a_registry_shorthand_typed_at_the_url_question_is_looked_up() {
    let dir = project();
    let mut script = Script::new([Answer::Text("npm:left-pad")]);

    let run = refs(dir.path(), &["add", "--no-sync"], Some(&mut script));

    assert_eq!(run.code, 1);
    assert!(script.rejected.is_empty(), "{:?}", script.rejected);
    assert!(run.err.contains("`left-pad` is not on npm"), "{}", run.err);
}
