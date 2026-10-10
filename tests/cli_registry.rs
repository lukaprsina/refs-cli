//! `add npm:`, `cargo:` and `pypi:` (ADR 0009) through `cli::run_on`, with a fake `Registry`
//! and a fake `Source`. The prefix is input sugar: `refs.toml` gets the expanded URL.

use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Terminal, run_on};
use refs_cli::prompt::{Abort, Prompter};
use refs_cli::registry::fake::FakeRegistry;
use refs_cli::registry::{Ecosystem, Found};
use refs_cli::source::fake::{Call, FakeSource};
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

const FOO: &str = "https://github.com/o/foo";

fn foo_registry() -> FakeRegistry {
    FakeRegistry::new().with(Ecosystem::Npm, "foo-js", found(FOO, None))
}

fn tags_asked(source: &FakeSource) -> usize {
    let calls = source.calls();
    calls.iter().filter(|c| matches!(c, Call::Tags(_))).count()
}

/// Answers every yes-or-no question with `yes`, and remembers the questions.
struct Confirms {
    yes: bool,
    asked: Vec<String>,
    texts: Vec<String>,
}

impl Prompter for Confirms {
    fn text(
        &mut self,
        message: &str,
        default: Option<&str>,
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        self.texts.push(message.to_owned());
        Defaults.text(message, default, validate)
    }

    fn suggest(
        &mut self,
        message: &str,
        suggestions: &[String],
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        Defaults.suggest(message, suggestions, validate)
    }

    fn confirm(&mut self, message: &str) -> Result<bool, Abort> {
        self.asked.push(message.to_owned());
        Ok(self.yes)
    }
}

#[test]
fn a_version_pins_the_tag_it_maps_to() {
    let dir = project();
    let source = FakeSource::new();
    source.set_tags(FOO, ["foo-js-1.1.0", "foo-js-1.2.3", "v1.2.3"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js@1.2.3", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    let config = config_text(dir.path());
    assert!(config.contains("ref = \"foo-js-1.2.3\""), "{config}");
    assert!(!config.contains("@1.2.3"), "{config}");
    assert!(config.contains("packages = [\"foo-js\"]"), "{config}");
}

#[test]
fn a_scoped_name_and_its_version_are_split_at_the_last_at() {
    let dir = project();
    let registry = FakeRegistry::new().with(Ecosystem::Npm, "@o/foo", found(FOO, None));
    let source = FakeSource::new();
    source.set_tags(FOO, ["@o/foo@2.0.0"]);

    let run = add(
        dir.path(),
        &registry,
        &source,
        &["npm:@o/foo@2.0.0", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        config_text(dir.path()).contains("ref = \"@o/foo@2.0.0\""),
        "{}",
        config_text(dir.path())
    );
}

#[test]
fn a_registry_add_with_no_version_pins_no_tag_and_lists_none() {
    let dir = project();
    let source = FakeSource::new();
    source.set_tags(FOO, ["v1.2.3"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(!config_text(dir.path()).contains("ref ="));
    assert_eq!(tags_asked(&source), 0);
}

#[test]
fn an_explicit_ref_wins_over_the_version() {
    let dir = project();
    let source = FakeSource::new();
    source.set_tags(FOO, ["v1.2.3"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js@1.2.3", "--id", "foo", "--ref", "main"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(dir.path()).contains("ref = \"main\""));
    assert_eq!(tags_asked(&source), 0);
}

#[test]
fn a_version_with_no_tag_without_a_terminal_warns_and_follows_head() {
    let dir = project();
    let source = FakeSource::new();
    source.set_tags(FOO, ["v1.0.0"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js@1.2.3", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains("warning") && run.err.contains("1.2.3") && run.err.contains("not tagged"),
        "{}",
        run.err
    );
    assert!(!config_text(dir.path()).contains("ref ="));
}

#[test]
fn a_version_with_no_tag_asks_and_continues_on_yes() {
    let dir = project();
    let source = FakeSource::new();
    let mut prompter = Confirms {
        yes: true,
        asked: vec![],
        texts: vec![],
    };

    let run = add_asking(
        dir.path(),
        &foo_registry(),
        &source,
        &[
            "npm:foo-js@1.2.3",
            "--id",
            "foo",
            "--group",
            "g",
            "--description",
            "d",
        ],
        Some(&mut prompter),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        prompter
            .asked
            .iter()
            .any(|q| q.contains("1.2.3") && q.contains("not tagged")),
        "{:?}",
        prompter.asked
    );
    assert!(dir.path().join("refs.lock").exists());
    // The person just said to follow the default branch: the Ref is not asked again.
    assert!(
        !prompter.texts.iter().any(|q| q.starts_with("Ref")),
        "{:?}",
        prompter.texts
    );
}

#[test]
fn a_bare_registry_add_does_not_ask_for_the_ref() {
    let dir = project();
    let source = FakeSource::new();
    let mut prompter = Confirms {
        yes: true,
        asked: vec![],
        texts: vec![],
    };

    let run = add_asking(
        dir.path(),
        &foo_registry(),
        &source,
        &[
            "npm:foo-js",
            "--id",
            "foo",
            "--group",
            "g",
            "--description",
            "d",
        ],
        Some(&mut prompter),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        !prompter.texts.iter().any(|q| q.starts_with("Ref")),
        "{:?}",
        prompter.texts
    );
}

#[test]
fn a_version_with_no_tag_cancels_on_no_and_changes_nothing() {
    let dir = project();
    let source = FakeSource::new();
    let mut prompter = Confirms {
        yes: false,
        asked: vec![],
        texts: vec![],
    };

    let run = add_asking(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js@1.2.3", "--id", "foo"],
        Some(&mut prompter),
    );

    assert_eq!(run.code, 1);
    assert!(run.err.contains("cancelled"), "{}", run.err);
    assert_eq!(config_text(dir.path()), "");
}

#[test]
fn no_sync_lists_no_tags_and_says_the_version_is_not_pinned() {
    let dir = project();
    let source = FakeSource::new();
    source.set_tags(FOO, ["v1.2.3"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js@1.2.3", "--id", "foo", "--no-sync"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains("warning") && run.err.contains("--no-sync"),
        "{}",
        run.err
    );
    assert!(!config_text(dir.path()).contains("ref ="));
    assert_eq!(tags_asked(&source), 0);
}

#[test]
fn a_version_that_is_empty_or_not_a_version_is_refused_before_the_registry_is_asked() {
    for shorthand in [
        "npm:foo-js@",
        "npm:foo-js@latest",
        "npm:foo-js@^1.2.0",
        "npm:foo-js@>=1",
        "npm:foo-js@*",
    ] {
        let dir = project();
        let registry = foo_registry();

        let run = add(dir.path(), &registry, &FakeSource::new(), &[shorthand]);

        assert_eq!(run.code, 1, "{shorthand}: {}", run.err);
        assert!(
            run.err.contains(shorthand) && run.err.contains("version"),
            "{shorthand}: {}",
            run.err
        );
        assert!(registry.calls().is_empty(), "{shorthand}");
        assert_eq!(config_text(dir.path()), "", "{shorthand}");
    }
}

#[test]
fn a_leading_v_on_the_version_is_ignored() {
    let dir = project();
    let source = FakeSource::new();
    source.set_tags(FOO, ["foo-js-1.2.3"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js@v1.2.3", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(dir.path()).contains("ref = \"foo-js-1.2.3\""));
}

// ---- the version a Package lockfile says the project uses (#64) ----

const NPM_LOCK_ONE: &str = r#"{"lockfileVersion":3,"packages":{
  "": {"dependencies":{"foo-js":"^1"}},
  "node_modules/foo-js":{"version":"1.2.3","resolved":"https://registry.npmjs.org/foo-js/-/foo-js-1.2.3.tgz"}}}"#;

/// Two versions of `foo-js`, neither named by the project.
const NPM_LOCK_TWO: &str = r#"{"lockfileVersion":3,"packages":{
  "": {"dependencies":{}},
  "node_modules/foo-js":{"version":"1.2.3","resolved":"https://registry.npmjs.org/foo-js/-/foo-js-1.2.3.tgz"},
  "node_modules/x/node_modules/foo-js":{"version":"1.10.0","resolved":"https://registry.npmjs.org/foo-js/-/foo-js-1.10.0.tgz"}}}"#;

/// Two versions of `foo-js`, one of them named by the project.
const NPM_LOCK_DIRECT: &str = r#"{"lockfileVersion":3,"packages":{
  "": {"dependencies":{"foo-js":"^1"}},
  "node_modules/foo-js":{"version":"1.2.3","resolved":"https://registry.npmjs.org/foo-js/-/foo-js-1.2.3.tgz"},
  "node_modules/x/node_modules/foo-js":{"version":"1.10.0","resolved":"https://registry.npmjs.org/foo-js/-/foo-js-1.10.0.tgz"}}}"#;

fn write(dir: &Path, name: &str, text: &str) {
    fs::write(dir.join(name), text).unwrap();
}

/// Answers the question about which version to use with `answer` (the default when `None`),
/// remembering the question and the default it came with, and takes the default of every other.
struct PicksVersion {
    answer: Option<&'static str>,
    asked: Vec<(String, Option<String>)>,
}

impl Prompter for PicksVersion {
    fn text(
        &mut self,
        message: &str,
        default: Option<&str>,
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        if !message.contains("versions") {
            return Defaults.text(message, default, validate);
        }
        self.asked
            .push((message.to_owned(), default.map(str::to_owned)));
        let answer = self.answer.or(default).unwrap_or("").to_owned();
        validate(&answer).map_err(|_| Abort::Cancelled)?;
        Ok(answer)
    }

    fn suggest(
        &mut self,
        message: &str,
        suggestions: &[String],
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        Defaults.suggest(message, suggestions, validate)
    }

    fn confirm(&mut self, message: &str) -> Result<bool, Abort> {
        Defaults.confirm(message)
    }
}

#[test]
fn the_version_in_the_lockfile_picks_its_tag_and_the_output_says_so() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_ONE);
    let source = FakeSource::new();
    source.set_tags(FOO, ["foo-js-1.1.0", "foo-js-1.2.3"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(dir.path()).contains("ref = \"foo-js-1.2.3\""));
    assert!(
        run.err.contains("package-lock.json") && run.err.contains("1.2.3"),
        "{}",
        run.err
    );
}

#[test]
fn a_cargo_shorthand_reads_cargo_lock() {
    let dir = project();
    write(
        dir.path(),
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"foo\"\nversion = \"0.4.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
    );
    let registry = FakeRegistry::new().with(Ecosystem::Cargo, "foo", found(FOO, None));
    let source = FakeSource::new();
    source.set_tags(FOO, ["v0.4.0", "v0.4.1"]);

    let run = add(
        dir.path(),
        &registry,
        &source,
        &["cargo:foo", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(dir.path()).contains("ref = \"v0.4.1\""));
    assert!(run.err.contains("Cargo.lock"), "{}", run.err);
}

#[test]
fn of_several_versions_the_one_the_project_names_is_used() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_DIRECT);
    let source = FakeSource::new();
    source.set_tags(FOO, ["v1.2.3", "v1.10.0"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(dir.path()).contains("ref = \"v1.2.3\""));
    assert!(run.err.contains("1.2.3"), "{}", run.err);
}

#[test]
fn of_several_versions_none_named_a_terminal_is_asked_with_the_highest_chosen() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_TWO);
    let source = FakeSource::new();
    source.set_tags(FOO, ["v1.2.3", "v1.10.0"]);
    let mut prompter = PicksVersion {
        answer: None,
        asked: vec![],
    };

    let run = add_asking(
        dir.path(),
        &foo_registry(),
        &source,
        &[
            "npm:foo-js",
            "--id",
            "foo",
            "--group",
            "g",
            "--description",
            "d",
        ],
        Some(&mut prompter),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    // 1.10.0 is higher than 1.2.3, which a comparison of the text would get wrong.
    assert_eq!(prompter.asked.len(), 1);
    assert_eq!(prompter.asked[0].1.as_deref(), Some("1.10.0"));
    assert!(
        prompter.asked[0].0.contains("1.2.3"),
        "{:?}",
        prompter.asked
    );
    assert!(config_text(dir.path()).contains("ref = \"v1.10.0\""));
}

#[test]
fn the_answer_to_the_question_picks_the_tag() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_TWO);
    let source = FakeSource::new();
    source.set_tags(FOO, ["v1.2.3", "v1.10.0"]);
    let mut prompter = PicksVersion {
        answer: Some("1.2.3"),
        asked: vec![],
    };

    let run = add_asking(
        dir.path(),
        &foo_registry(),
        &source,
        &[
            "npm:foo-js",
            "--id",
            "foo",
            "--group",
            "g",
            "--description",
            "d",
        ],
        Some(&mut prompter),
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(dir.path()).contains("ref = \"v1.2.3\""));
}

#[test]
fn an_answer_that_is_not_one_of_the_versions_is_refused_by_the_question() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_TWO);
    let mut prompter = PicksVersion {
        answer: Some("9.9.9"),
        asked: vec![],
    };

    let run = add_asking(
        dir.path(),
        &foo_registry(),
        &FakeSource::new(),
        &["npm:foo-js", "--id", "foo"],
        Some(&mut prompter),
    );

    assert_eq!(run.code, 1);
    assert_eq!(config_text(dir.path()), "");
}

#[test]
fn of_several_versions_none_named_no_terminal_refuses_and_says_to_give_a_version() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_TWO);
    let source = FakeSource::new();

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js", "--id", "foo"],
    );

    assert_eq!(run.code, 1);
    assert!(
        run.err.contains("1.2.3") && run.err.contains("1.10.0") && run.err.contains("@version"),
        "{}",
        run.err
    );
    assert_eq!(config_text(dir.path()), "");
}

#[test]
fn a_package_the_lockfile_lacks_follows_the_default_branch_and_says_why() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_ONE);
    let registry = FakeRegistry::new().with(Ecosystem::Npm, "bar", found(FOO, None));
    let source = FakeSource::new();

    let run = add(dir.path(), &registry, &source, &["npm:bar", "--id", "foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains("bar") && run.err.contains("package-lock.json"),
        "{}",
        run.err
    );
    assert!(!config_text(dir.path()).contains("ref ="));
    assert_eq!(tags_asked(&source), 0);
}

#[test]
fn no_lockfile_follows_the_default_branch_and_says_why() {
    let dir = project();
    let source = FakeSource::new();

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("lockfile"), "{}", run.err);
    assert!(!config_text(dir.path()).contains("ref ="));
}

#[test]
fn a_lockfile_that_cannot_be_read_warns_and_follows_the_default_branch() {
    let dir = project();
    write(dir.path(), "package-lock.json", "not json");

    let run = add(
        dir.path(),
        &foo_registry(),
        &FakeSource::new(),
        &["npm:foo-js", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains("warning") && run.err.contains("package-lock.json"),
        "{}",
        run.err
    );
    assert!(!config_text(dir.path()).contains("ref ="));
}

#[test]
fn a_given_version_or_ref_skips_the_lockfile() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_ONE);
    let source = FakeSource::new();
    source.set_tags(FOO, ["v1.2.3", "v1.0.0"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js@1.0.0", "--id", "foo"],
    );
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(dir.path()).contains("ref = \"v1.0.0\""));
    assert!(!run.err.contains("package-lock.json"), "{}", run.err);

    let other = project();
    write(other.path(), "package-lock.json", NPM_LOCK_ONE);
    let run = add(
        other.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js", "--id", "foo", "--ref", "main"],
    );
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(config_text(other.path()).contains("ref = \"main\""));
    assert!(!run.err.contains("package-lock.json"), "{}", run.err);
}

#[test]
fn a_pypi_shorthand_reads_no_lockfile() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_ONE);
    let registry = FakeRegistry::new().with(Ecosystem::Pypi, "foo-js", found(FOO, None));

    let run = add(
        dir.path(),
        &registry,
        &FakeSource::new(),
        &["pypi:foo-js", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(!run.err.contains("lockfile"), "{}", run.err);
    assert!(!config_text(dir.path()).contains("ref ="));
}

#[test]
fn the_lockfile_version_with_no_tag_is_a_miss_like_any_other() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_ONE);
    let source = FakeSource::new();
    source.set_tags(FOO, ["v9.9.9"]);

    let run = add(
        dir.path(),
        &foo_registry(),
        &source,
        &["npm:foo-js", "--id", "foo"],
    );

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("not tagged"), "{}", run.err);
    assert!(!config_text(dir.path()).contains("ref ="));
}

#[test]
fn offline_still_refuses_the_registry_lookup_with_a_lockfile() {
    let dir = project();
    write(dir.path(), "package-lock.json", NPM_LOCK_ONE);

    let run = add(
        dir.path(),
        &foo_registry(),
        &FakeSource::new(),
        &["npm:foo-js", "--offline", "--no-sync"],
    );

    assert_eq!(run.code, 1);
    assert!(run.err.contains("--offline"), "{}", run.err);
}
