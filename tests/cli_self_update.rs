//! `refs self update` through `cli::run_on`, with the updater scripted: nothing here touches the
//! network or the installed binary. The `Source` factory panics, as `self update` needs no project
//! and no checkouts.

use std::path::Path;

use refs_cli::cli::{Outside, Styled, run_on};
use refs_cli::diagnostic::SourceError;
use refs_cli::self_update::Updater;
use refs_cli::self_update::fake::{Call, FakeUpdater, Unmanaged};
use refs_cli::source::Source;
use tempfile::TempDir;

struct Run {
    code: u8,
    out: String,
    err: String,
}

fn refs(dir: &Path, args: &[&str], updater: &dyn Updater) -> Run {
    let args = std::iter::once("refs").chain(args.iter().copied());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_on(
        args.map(Into::into),
        dir,
        |_, _| -> Result<Box<dyn Source>, SourceError> {
            panic!("`self update` asked for a Source")
        },
        &mut out,
        &mut err,
        Styled::default(),
        Outside {
            prompter: None,
            updater,
            registry: &refs_cli::registry::fake::Unused,
        },
    );
    Run {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn self_update_installs_the_newer_release_and_says_which() {
    let dir = TempDir::new().unwrap();
    let updater = FakeUpdater::release("0.1.0", "0.2.0");

    let run = refs(dir.path(), &["self", "update"], &updater);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.err, "updated refs from 0.1.0 to 0.2.0\n");
    assert_eq!(run.out, "");
    assert_eq!(updater.calls(), [Call::Install]);
}

#[test]
fn self_update_when_already_current_says_so() {
    let dir = TempDir::new().unwrap();
    let updater = FakeUpdater::current();

    let run = refs(dir.path(), &["self", "update"], &updater);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.err, "up to date\n");
    assert_eq!(run.out, "");
}

#[test]
fn quiet_silences_the_self_update_status_line() {
    let dir = TempDir::new().unwrap();

    let run = refs(
        dir.path(),
        &["-q", "self", "update"],
        &FakeUpdater::release("0.1.0", "0.2.0"),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.err, "");
}

#[test]
fn self_update_check_exits_3_when_a_newer_release_exists_and_installs_nothing() {
    let dir = TempDir::new().unwrap();
    let updater = FakeUpdater::release("0.1.0", "0.2.0");

    let run = refs(dir.path(), &["self", "update", "--check"], &updater);

    assert_eq!(run.code, 3, "{}", run.err);
    assert_eq!(
        run.err,
        "out of date: a newer release is available; run `refs self update`\n"
    );
    assert_eq!(run.out, "");
    assert_eq!(updater.calls(), [Call::Check]);
}

#[test]
fn self_update_check_on_the_latest_release_is_up_to_date() {
    let dir = TempDir::new().unwrap();
    let updater = FakeUpdater::current();

    let run = refs(dir.path(), &["self", "update", "--check"], &updater);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.err, "up to date\n");
    assert_eq!(updater.calls(), [Call::Check]);
}

#[test]
fn self_update_on_a_binary_the_installer_did_not_put_there_says_how_to_update() {
    let dir = TempDir::new().unwrap();

    for args in [&["self", "update"][..], &["self", "update", "--check"]] {
        let run = refs(dir.path(), args, &Unmanaged);

        assert_eq!(run.code, 1, "{args:?}");
        assert!(
            run.err
                .contains("not installed with the cargo-dist installer"),
            "{}",
            run.err
        );
        assert!(
            run.err.contains("update it the way you installed it"),
            "{}",
            run.err
        );
        assert_eq!(run.out, "");
    }
}

#[test]
fn a_failed_self_update_is_an_error_even_with_quiet() {
    let dir = TempDir::new().unwrap();

    let run = refs(
        dir.path(),
        &["-q", "self", "update"],
        &FakeUpdater::failing("connection refused"),
    );

    assert_eq!(run.code, 1);
    assert!(
        run.err.contains("could not update: connection refused"),
        "{}",
        run.err
    );
}

#[test]
fn the_old_top_level_update_call_is_the_repo_update_and_installs_nothing() {
    let dir = TempDir::new().unwrap();
    let updater = FakeUpdater::release("0.1.0", "0.2.0");

    let run = refs(dir.path(), &["update"], &updater);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("refs.toml"), "{}", run.err);
    assert_eq!(updater.calls(), []);
}
