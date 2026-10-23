//! `refs update <id>…` (ADR 0012) through `cli::run_on`, with a fake `Source` and a `Registry`
//! that fails if it is asked: `update` never asks the Registry. Package lockfiles are real files.

use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Terminal, run_on};
use refs_cli::prompt::Prompter;
use refs_cli::registry::Unavailable;
use refs_cli::self_update::Unmanaged;
use refs_cli::source::Observed;
use refs_cli::source::Source;
use refs_cli::source::fake::{Call, FakeSource, Method};
use tempfile::TempDir;

mod common;
use common::script::{Answer, Script};

const FOO: &str = "https://github.com/o/foo";
const BAR: &str = "https://github.com/o/bar";
const SHA_1: &str = "1111111111111111111111111111111111111111";
const SHA_2: &str = "2222222222222222222222222222222222222222";

const NPM_LOCK_ONE: &str = r#"{"lockfileVersion":3,"packages":{
  "": {"dependencies":{"foo-js":"^1"}},
  "node_modules/foo-js":{"version":"1.2.3","resolved":"https://registry.npmjs.org/foo-js/-/foo-js-1.2.3.tgz"}}}"#;

const NPM_LOCK_TWO: &str = r#"{"lockfileVersion":3,"packages":{
  "": {"dependencies":{}},
  "node_modules/foo-js":{"version":"1.2.3","resolved":"https://registry.npmjs.org/foo-js/-/foo-js-1.2.3.tgz"},
  "node_modules/x/node_modules/foo-js":{"version":"1.10.0","resolved":"https://registry.npmjs.org/foo-js/-/foo-js-1.10.0.tgz"}}}"#;

const NPM_LOCK_WITHOUT_FOO: &str = r#"{"lockfileVersion":3,"packages":{
  "": {"dependencies":{"other":"^1"}},
  "node_modules/other":{"version":"1.0.0","resolved":"https://registry.npmjs.org/other/-/other-1.0.0.tgz"}}}"#;

/// `foo` pinned to a Version tag, with the Origin it was added with.
fn foo_at(git_ref: &str) -> String {
    format!(
        "# the docs\n[repos.foo]\nurl = \"{FOO}\"\nref = \"{git_ref}\" # pinned\ndescription = \"kept\"\norigin = \"npm:foo-js\"\n"
    )
}

struct Run {
    code: u8,
    err: String,
}

/// A project on disk and the fake remote its runs share.
struct Project {
    dir: TempDir,
    source: FakeSource,
}

impl Project {
    /// The project with `config`, locked and synced once.
    fn new(config: &str) -> Project {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("refs.toml"), config).unwrap();
        let project = Project {
            dir,
            source: FakeSource::new(),
        };
        let first = project.run(&["sync"], None);
        assert_eq!(first.code, 0, "{}", first.err);
        project
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.path().join(name)).unwrap_or_default()
    }

    fn write(&self, name: &str, text: &str) {
        fs::write(self.path().join(name), text).unwrap();
    }

    fn run(&self, args: &[&str], prompter: Option<&mut dyn Prompter>) -> Run {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_on(
            std::iter::once("refs")
                .chain(args.iter().copied())
                .map(Into::into),
            self.path(),
            |_, _| Ok(Box::new(&self.source)),
            &mut out,
            &mut err,
            Terminal::default(),
            Outside {
                prompter,
                updater: &Unmanaged,
                registry: &Unavailable,
            },
        );
        Run {
            code,
            err: String::from_utf8(err).unwrap(),
        }
    }

    fn update(&self, args: &[&str]) -> Run {
        let args: Vec<&str> = ["update"].into_iter().chain(args.iter().copied()).collect();
        self.run(&args, None)
    }
}

// ---- --ref ----

#[test]
fn ref_moves_a_repo_and_keeps_the_rest_of_refs_toml() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));

    let run = p.update(&["foo", "--ref", "foo-js-2.0.0"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        p.read("refs.toml"),
        "# the docs
[repos.foo]
url = \"https://github.com/o/foo\"
ref = \"foo-js-2.0.0\" # pinned
description = \"kept\"
origin = \"npm:foo-js\"
"
    );
    assert!(p.read("refs.lock").contains("ref = \"foo-js-2.0.0\""));
    assert!(
        run.err.contains("foo  foo-js-1.0.0 → foo-js-2.0.0"),
        "{}",
        run.err
    );
}

#[test]
fn a_ref_the_remote_lacks_skips_that_repo_and_writes_nothing() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.source.fail(FOO, Method::Outline, "no such ref");
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let run = p.update(&["foo", "--ref", "nope"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(
        run.err.contains("`foo`") && run.err.contains("no such ref"),
        "{}",
        run.err
    );
    assert!(run.err.contains("1 repo was not updated"), "{}", run.err);
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn ref_with_several_ids_is_a_usage_error() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));

    let run = p.update(&["foo", "bar", "--ref", "v1"]);

    assert_eq!(run.code, 2, "{}", run.err);
}

// ---- news from the Package lockfile ----

fn with_tags(p: &Project, tags: &[&str]) {
    p.source.set_tags(FOO, tags.iter().copied());
}

#[test]
fn a_pin_moves_to_the_version_the_lockfile_uses() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        p.read("refs.toml")
            .contains("ref = \"foo-js-1.2.3\" # pinned")
    );
    assert!(
        run.err.contains("foo  foo-js-1.0.0 → foo-js-1.2.3"),
        "{}",
        run.err
    );
    assert!(p.read("refs.toml").contains("description = \"kept\""));
}

#[test]
fn a_downgrade_is_a_move_too() {
    let p = Project::new(&foo_at("foo-js-2.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.2.3", "foo-js-2.0.0"]);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains("foo  foo-js-2.0.0 → foo-js-1.2.3"),
        "{}",
        run.err
    );
}

#[test]
fn a_pin_that_already_matches_the_lockfile_changes_nothing() {
    let p = Project::new(&foo_at("foo-js-1.2.3"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.2.3"]);
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("`foo` is up to date"), "{}", run.err);
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn a_version_with_no_tag_is_a_note_and_the_repo_stays() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0"]);
    let before = p.read("refs.toml");

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains("foo-js@1.2.3 is not tagged"),
        "{}",
        run.err
    );
    assert_eq!(p.read("refs.toml"), before);
}

#[test]
fn a_package_the_lockfile_lacks_or_no_lockfile_is_a_note() {
    for lockfile in [Some(NPM_LOCK_WITHOUT_FOO), None] {
        let p = Project::new(&foo_at("foo-js-1.0.0"));
        if let Some(lockfile) = lockfile {
            p.write("package-lock.json", lockfile);
        }
        with_tags(&p, &["foo-js-1.0.0"]);
        let before = p.read("refs.toml");

        let run = p.update(&["foo"]);

        assert_eq!(run.code, 0, "{}", run.err);
        assert!(run.err.contains("`foo` is up to date"), "{}", run.err);
        assert!(run.err.contains("foo-js"), "{}", run.err);
        assert_eq!(p.read("refs.toml"), before);
    }
}

#[test]
fn a_pinned_repo_with_no_origin_is_pointed_at_ref() {
    let config = foo_at("foo-js-1.0.0").replace("origin = \"npm:foo-js\"\n", "");
    let p = Project::new(&config);
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("`--ref`"), "{}", run.err);
    assert_eq!(p.read("refs.toml"), config);
}

#[test]
fn a_full_commit_is_pointed_at_ref_even_with_an_origin() {
    let config = foo_at(SHA_1);
    let p = Project::new(&config);
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.2.3"]);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("`--ref`"), "{}", run.err);
    assert_eq!(p.read("refs.toml"), config);
}

#[test]
fn several_versions_ask_which_and_without_a_terminal_skip_the_repo() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_TWO);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3", "foo-js-1.10.0"]);

    let skipped = p.update(&["foo"]);
    assert_eq!(skipped.code, 1, "{}", skipped.err);
    assert!(skipped.err.contains("1.2.3, 1.10.0"), "{}", skipped.err);

    let mut prompter = Script::new([Answer::Text("1.2.3")]);
    let asked = p.run(&["update", "foo"], Some(&mut prompter));
    assert_eq!(asked.code, 0, "{}", asked.err);
    assert!(p.read("refs.toml").contains("ref = \"foo-js-1.2.3\""));
}

// ---- floating Refs ----

const BRANCH: &str = "[repos.foo]\nurl = \"https://github.com/o/foo\"\nref = \"main\"\n";

#[test]
fn a_branch_moves_in_the_lock_and_refs_toml_is_untouched() {
    let p = Project::new(BRANCH);
    p.source.set_commit("foo", SHA_1);
    assert_eq!(p.run(&["lock", "--upgrade"], None).code, 0);
    p.source.set_commit("foo", SHA_2);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(p.read("refs.toml"), BRANCH);
    assert!(p.read("refs.lock").contains(SHA_2));
    assert!(
        run.err.contains("foo  1111111 → 2222222 (main)"),
        "{}",
        run.err
    );
}

#[test]
fn a_branch_whose_commit_did_not_change_stays() {
    let p = Project::new(BRANCH);
    let lock = p.read("refs.lock");

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("`foo` is up to date"), "{}", run.err);
    assert_eq!(p.read("refs.lock"), lock);
}

// ---- dirty Checkouts ----

fn make_dirty(p: &Project, id: &str) {
    let Observed::At { pin, paths, .. } = p.source.inspect(id).unwrap() else {
        panic!("{id} is checked out");
    };
    p.source.seed(
        id,
        Observed::At {
            pin,
            paths,
            dirty_files: vec!["notes.txt".into()],
        },
    );
}

#[test]
fn a_dirty_mover_is_skipped_with_its_files_and_force_moves_it() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    make_dirty(&p, "foo");
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let skipped = p.update(&["foo", "--ref", "foo-js-2.0.0"]);

    assert_eq!(skipped.code, 1, "{}", skipped.err);
    assert!(skipped.err.contains("notes.txt"), "{}", skipped.err);
    assert!(
        skipped.err.contains("1 repo was not updated"),
        "{}",
        skipped.err
    );
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);

    let forced = p.update(&["foo", "--ref", "foo-js-2.0.0", "--force"]);
    assert_eq!(forced.code, 0, "{}", forced.err);
    assert!(p.read("refs.toml").contains("foo-js-2.0.0"));
}

// ---- several Repos ----

const BAZ: &str = "https://github.com/o/baz";

const THREE: &str = "[repos.foo]
url = \"https://github.com/o/foo\"
ref = \"foo-js-1.0.0\"
origin = \"npm:foo-js\"

[repos.bar]
url = \"https://github.com/o/bar\"
ref = \"main\"

[repos.baz]
url = \"https://github.com/o/baz\"
ref = \"v1\"
origin = \"npm:baz\"
";

#[test]
fn several_repos_make_one_edit_and_one_sync_and_a_broken_one_exits_1() {
    let p = Project::new(THREE);
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    p.source.set_tags(BAR, ["v1"]);
    p.source.set_commit("bar", SHA_2);
    p.source.fail(BAZ, Method::Tags, "remote down");
    let calls = p.source.calls().len();

    let run = p.update(&["foo", "bar", "baz"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert_eq!(
        p.read("refs.toml"),
        THREE.replace("ref = \"foo-js-1.0.0\"", "ref = \"foo-js-1.2.3\"")
    );
    assert!(p.read("refs.lock").contains(SHA_2));
    assert!(
        run.err.contains("`baz`") && run.err.contains("remote down"),
        "{}",
        run.err
    );
    assert!(run.err.contains("1 repo was not updated"), "{}", run.err);
    assert_eq!(
        run.err.matches("updated refs.toml").count(),
        1,
        "{}",
        run.err
    );
    assert_eq!(
        run.err.matches("updated refs.lock").count(),
        1,
        "{}",
        run.err
    );
    let verified_foo = p.source.calls()[calls..]
        .iter()
        .filter(|c| matches!(c, Call::Verify { id, .. } if id == "foo"))
        .count();
    assert_eq!(verified_foo, 1);
}

// ---- refusals before any network call ----

#[test]
fn unknown_and_disabled_ids_refuse_the_whole_run_before_the_network() {
    let config =
        format!("{THREE}\n[repos.off]\nurl = \"https://github.com/o/off\"\nenabled = false\n");
    let p = Project::new(&config);
    let calls = p.source.calls().len();

    let run = p.update(&["foo", "nope", "off"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(
        run.err.contains("nope") && run.err.contains("off"),
        "{}",
        run.err
    );
    assert_eq!(p.source.calls().len(), calls);
}

#[test]
fn update_is_refused_offline() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    let calls = p.source.calls().len();

    let run = p.run(&["--offline", "update", "foo", "--ref", "v2"], None);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("network"), "{}", run.err);
    assert_eq!(p.source.calls().len(), calls);
}

#[test]
fn update_without_ids_is_a_usage_error() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));

    assert_eq!(p.update(&[]).code, 2);
}

#[test]
fn quiet_hides_the_notes_but_not_the_summary_of_skips() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    with_tags(&p, &["foo-js-1.0.0"]);
    let noted = p.run(&["-q", "update", "foo"], None);
    assert_eq!((noted.code, noted.err.as_str()), (0, ""));

    p.source.fail(FOO, Method::Outline, "no such ref");
    let skipped = p.run(&["-q", "update", "foo", "--ref", "nope"], None);

    assert_eq!(skipped.code, 1);
    assert!(
        skipped.err.contains("1 repo was not updated"),
        "{}",
        skipped.err
    );
}

#[test]
fn cancelling_the_version_question_edits_nothing() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_TWO);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    let before = (p.read("refs.toml"), p.read("refs.lock"));
    let mut prompter = Script::new([Answer::Cancel]);

    let run = p.run(&["update", "foo"], Some(&mut prompter));

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("cancelled"), "{}", run.err);
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}
