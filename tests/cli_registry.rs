//! `add npm:`, `cargo:` and `pypi:` (ADR 0009) through `cli::run_on`, with a fake `Registry`
//! and a fake `Source`. The prefix is input sugar: `refs.toml` gets the expanded URL.

use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Terminal, run_on};
use refs_cli::prompt::{Abort, Prompter};
use refs_cli::registry::fake::FakeRegistry;
use refs_cli::registry::{Ecosystem, Found};
use refs_cli::source::fake::FakeSource;
use refs_cli::update::Unmanaged;
use tempfile::TempDir;

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

fn found(url: &str, directory: Option<&str>) -> Found {
    Found {
        url: url.into(),
        directory: directory.map(Into::into),
    }
}

/// Takes every default and says no to every yes-or-no question.
struct Defaults;

impl Prompter for Defaults {
    fn text(
        &mut self,
        _message: &str,
        default: Option<&str>,
        _validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        Ok(default.unwrap_or("").to_owned())
    }

    fn suggest(
        &mut self,
        _message: &str,
        _suggestions: &[String],
        _validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        Ok(String::new())
    }

    fn confirm(&mut self, _message: &str) -> Result<bool, Abort> {
        Ok(false)
    }
}

/// `refs add <args>` with no terminal, so nothing is asked.
fn add(dir: &Path, registry: &FakeRegistry, source: &FakeSource, args: &[&str]) -> Run {
    add_asking(dir, registry, source, args, None)
}

fn add_asking<'a>(
    dir: &Path,
    registry: &'a FakeRegistry,
    source: &FakeSource,
    args: &[&str],
    prompter: Option<&'a mut dyn Prompter>,
) -> Run {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_on(
        ["refs", "add"]
            .into_iter()
            .chain(args.iter().copied())
            .map(Into::into),
        dir,
        |_, _| Ok(Box::new(source)),
        &mut out,
        &mut err,
        Terminal::default(),
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

#[test]
fn an_npm_shorthand_writes_the_expanded_url_the_directory_and_the_name() {
    let dir = project();
    let registry = FakeRegistry::new().with(
        Ecosystem::Npm,
        "@o/foo-js",
        found("https://github.com/o/foo", Some("packages/foo")),
    );

    let run = add(
        dir.path(),
        &registry,
        &FakeSource::new(),
        &["npm:@o/foo-js", "--no-sync"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(
        config.contains("url = \"https://github.com/o/foo\""),
        "{config}"
    );
    assert!(config.contains("paths = [\"packages/foo\"]"), "{config}");
    assert!(config.contains("packages = [\"@o/foo-js\"]"), "{config}");
    assert!(!config.contains("npm:"), "{config}");
}

#[test]
fn cargo_and_pypi_shorthands_write_the_url_and_the_name() {
    for (prefix, ecosystem) in [("cargo", Ecosystem::Cargo), ("pypi", Ecosystem::Pypi)] {
        let dir = project();
        let registry =
            FakeRegistry::new().with(ecosystem, "foo", found("https://github.com/o/foo", None));

        let run = add(
            dir.path(),
            &registry,
            &FakeSource::new(),
            &[&format!("{prefix}:foo"), "--no-sync"],
        );

        assert_eq!(run.code, 0, "{}", run.err);
        let config = config_text(dir.path());
        assert!(
            config.contains("url = \"https://github.com/o/foo\""),
            "{config}"
        );
        assert!(config.contains("packages = [\"foo\"]"), "{config}");
        assert!(!config.contains("paths"), "{config}");
    }
}

#[test]
fn explicit_paths_and_packages_win_over_the_registry() {
    let dir = project();
    let registry = FakeRegistry::new().with(
        Ecosystem::Npm,
        "foo-js",
        found("https://github.com/o/foo", Some("packages/foo")),
    );

    let run = add(
        dir.path(),
        &registry,
        &FakeSource::new(),
        &[
            "npm:foo-js",
            "--paths",
            "docs",
            "--packages",
            "mine",
            "--no-sync",
        ],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(config.contains("paths = [\"docs\"]"), "{config}");
    assert!(config.contains("packages = [\"mine\"]"), "{config}");
}

#[test]
fn an_unknown_package_is_a_clear_error_and_changes_nothing() {
    let dir = project();

    let run = add(
        dir.path(),
        &FakeRegistry::new(),
        &FakeSource::new(),
        &["npm:nope", "--no-sync"],
    );

    assert_eq!(run.code, 1);
    assert!(
        run.err.contains("`nope`") && run.err.contains("npm"),
        "{}",
        run.err
    );
    assert_eq!(config_text(dir.path()), "");
}

#[test]
fn offline_refuses_the_lookup() {
    let dir = project();
    let registry = FakeRegistry::new().with(
        Ecosystem::Npm,
        "foo-js",
        found("https://github.com/o/foo", None),
    );

    let run = add(
        dir.path(),
        &registry,
        &FakeSource::new(),
        &["npm:foo-js", "--offline", "--no-sync"],
    );

    assert_eq!(run.code, 1);
    assert!(run.err.contains("--offline"), "{}", run.err);
    assert!(registry.calls().is_empty());
    assert_eq!(config_text(dir.path()), "");
}

#[test]
fn a_url_a_hand_typed_one_would_be_rejected_for_is_rejected() {
    let dir = project();
    let registry = FakeRegistry::new().with(
        Ecosystem::Npm,
        "foo-js",
        found("http://github.com/o/foo", None),
    );

    let run = add(
        dir.path(),
        &registry,
        &FakeSource::new(),
        &["npm:foo-js", "--no-sync"],
    );

    assert_eq!(run.code, 1);
    assert!(run.err.contains("http://github.com/o/foo"), "{}", run.err);
    assert_eq!(config_text(dir.path()), "");
}

#[test]
fn with_a_sync_the_checkout_is_made_and_nothing_is_inferred() {
    let dir = project();
    let registry = FakeRegistry::new().with(
        Ecosystem::Npm,
        "foo-js",
        found("https://github.com/o/foo", None),
    );
    let source = FakeSource::new();

    let run = add(
        dir.path(),
        &registry,
        &source,
        &["npm:foo-js", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(
        config.contains("[repos.foo]") && config.contains("packages = [\"foo-js\"]"),
        "{config}"
    );
    assert_eq!(registry.calls().len(), 1);
    assert!(dir.path().join("refs.lock").exists());
}

#[test]
fn the_equivalent_command_has_the_expanded_url_and_the_registrys_packages() {
    let dir = project();
    let registry = FakeRegistry::new().with(
        Ecosystem::Npm,
        "foo-js",
        found("https://github.com/o/foo", Some("packages/foo")),
    );

    let run = add_asking(
        dir.path(),
        &registry,
        &FakeSource::new(),
        &["npm:foo-js", "--id", "foo"],
        Some(&mut Defaults),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    let line = run.err.lines().find(|l| l.starts_with("equivalent:"));
    assert!(
        line.is_some_and(|l| l.contains("https://github.com/o/foo")
            && l.contains("--paths packages/foo")
            && l.contains("--packages foo-js")
            && !l.contains("npm:")),
        "{}",
        run.err
    );
}
