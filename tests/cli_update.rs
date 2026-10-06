//! `refs update` through `cli::run_on`, with the updater scripted: nothing here touches the
//! network or the installed binary. The `Source` factory panics, as `update` needs no project
//! and no checkouts.

use std::path::Path;

use refs_cli::cli::{Outside, Terminal, run_on};
use refs_cli::diagnostic::SourceError;
use refs_cli::source::Source;
use refs_cli::update::fake::{Call, FakeUpdater};
use tempfile::TempDir;

struct Run {
    code: u8,
    out: String,
    err: String,
}

fn refs(dir: &Path, args: &[&str], updater: &FakeUpdater) -> Run {
    let args = std::iter::once("refs").chain(args.iter().copied());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_on(
        args.map(Into::into),
        dir,
        |_, _| -> Result<Box<dyn Source>, SourceError> { panic!("`update` asked for a Source") },
        &mut out,
        &mut err,
        Terminal::default(),
        Outside {
            prompter: None,
            updater,
        },
    );
    Run {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn update_installs_the_newer_release_and_says_which() {
    let dir = TempDir::new().unwrap();
    let updater = FakeUpdater::release("0.1.0", "0.2.0");

    let run = refs(dir.path(), &["update"], &updater);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.err, "updated refs from 0.1.0 to 0.2.0\n");
    assert_eq!(run.out, "");
    assert_eq!(updater.calls(), [Call::Install]);
}

#[test]
fn update_when_already_current_says_so() {
    let dir = TempDir::new().unwrap();
    let updater = FakeUpdater::current();

    let run = refs(dir.path(), &["update"], &updater);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.err, "up to date\n");
    assert_eq!(run.out, "");
}

#[test]
fn quiet_silences_the_status_line() {
    let dir = TempDir::new().unwrap();

    let run = refs(
        dir.path(),
        &["-q", "update"],
        &FakeUpdater::release("0.1.0", "0.2.0"),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.err, "");
}

#[test]
fn check_exits_3_when_a_newer_release_exists_and_installs_nothing() {
    let dir = TempDir::new().unwrap();
    let updater = FakeUpdater::release("0.1.0", "0.2.0");

    let run = refs(dir.path(), &["update", "--check"], &updater);

    assert_eq!(run.code, 3, "{}", run.err);
    assert_eq!(
        run.err,
        "out of date: a newer release is available; run `refs update`\n"
    );
    assert_eq!(run.out, "");
    assert_eq!(updater.calls(), [Call::Check]);
}

#[test]
fn check_on_the_latest_release_is_up_to_date() {
    let dir = TempDir::new().unwrap();
    let updater = FakeUpdater::current();

    let run = refs(dir.path(), &["update", "--check"], &updater);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.err, "up to date\n");
    assert_eq!(updater.calls(), [Call::Check]);
}

#[test]
fn a_binary_the_installer_did_not_put_there_is_told_how_to_update() {
    let dir = TempDir::new().unwrap();

    for args in [&["update"][..], &["update", "--check"]] {
        let run = refs(dir.path(), args, &FakeUpdater::unmanaged());

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
fn a_failed_update_is_an_error_even_with_quiet() {
    let dir = TempDir::new().unwrap();

    let run = refs(
        dir.path(),
        &["-q", "update"],
        &FakeUpdater::failing("connection refused"),
    );

    assert_eq!(run.code, 1);
    assert!(
        run.err.contains("could not update: connection refused"),
        "{}",
        run.err
    );
}
