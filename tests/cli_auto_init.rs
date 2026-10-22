//! `refs add` where there is no `refs.toml` yet: the edit creates one (ADR 0005, 0011), at the
//! top of the git worktree, or where it ran outside git.

use std::fs;
use std::path::Path;
use std::process::Command;

use refs_cli::cli::{Outside, Terminal, run_on};
use refs_cli::prompt::Prompter;
use refs_cli::source::fake::FakeSource;
use refs_cli::update::Unmanaged;
use tempfile::TempDir;

mod common;
use common::script::{Answer, Script};

struct Run {
    code: u8,
    err: String,
}

fn refs(dir: &Path, args: &[&str], prompter: Option<&mut dyn Prompter>) -> Run {
    let args = std::iter::once("refs").chain(args.iter().copied());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let source = FakeSource::new();
    let code = run_on(
        args.map(Into::into),
        dir,
        |_, _| Ok(Box::new(&source)),
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

const ADD: [&str; 4] = ["add", "https://github.com/o/lib", "--no-sync", "--no-input"];

#[test]
fn outside_git_the_synced_edit_creates_refs_toml_here_and_warns_about_the_exclude_rule() {
    let dir = TempDir::new().unwrap();

    let run = refs(
        dir.path(),
        &["add", "https://github.com/o/lib", "--no-input"],
        None,
    );

    assert_eq!(run.code, 0, "{}", run.err);
    let config = fs::read_to_string(dir.path().join("refs.toml")).unwrap();
    assert!(config.contains("[repos.lib]"), "{config}");
    assert!(
        !config.contains('#'),
        "an empty file, not the template: {config}"
    );
    assert!(
        run.err.contains("created") && run.err.contains("refs.toml"),
        "{}",
        run.err
    );
    assert!(run.err.contains("not a git repository"), "{}", run.err);
}

#[test]
fn in_a_git_worktree_it_goes_to_the_top_from_a_subdirectory() {
    let dir = TempDir::new().unwrap();
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status();
    assert!(git.unwrap().success());
    let sub = dir.path().join("a/b");
    fs::create_dir_all(&sub).unwrap();

    let run = refs(&sub, &ADD, None);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(dir.path().join("refs.toml").is_file());
    assert!(!sub.join("refs.toml").exists());
    assert!(!run.err.contains("not a git repository"), "{}", run.err);
}

#[test]
fn a_cancelled_add_creates_nothing() {
    let dir = TempDir::new().unwrap();
    let mut script = Script::new([Answer::Cancel]);

    let run = refs(
        dir.path(),
        &["add", "https://github.com/o/lib", "--no-sync"],
        Some(&mut script),
    );

    assert_eq!(run.code, 1);
    assert!(!dir.path().join("refs.toml").exists());
}

#[test]
fn other_commands_still_need_a_refs_toml() {
    let dir = TempDir::new().unwrap();

    let run = refs(dir.path(), &["list"], None);

    assert_eq!(run.code, 1);
    assert!(!dir.path().join("refs.toml").exists());
}

#[test]
fn init_outside_git_warns_and_still_writes_the_template() {
    let dir = TempDir::new().unwrap();

    let run = refs(dir.path(), &["init"], None);

    assert_eq!(run.code, 0, "{}", run.err);
    let config = fs::read_to_string(dir.path().join("refs.toml")).unwrap();
    assert!(config.contains("# [repos.solid]"), "{config}");
    assert!(run.err.contains("not a git repository"), "{}", run.err);
}
