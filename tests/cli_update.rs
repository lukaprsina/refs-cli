//! `refs update <id>…` (ADR 0012) through `cli::run_on`, with a fake `Source` and a `Registry`
//! that fails if it is asked: `update` never asks the Registry. Package lockfiles are real files.

use std::fs;
use std::path::Path;

use refs_cli::cli::{Outside, Styled, run_on};
use refs_cli::source::Observed;
use refs_cli::source::Source;
use refs_cli::source::fake::{FakeSource, Method};
use refs_cli::ui::Prompter;
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
    out: String,
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
            |_, _| Ok(Box::new(self.source.clone())),
            &mut out,
            &mut err,
            Styled::default(),
            Outside::offline(prompter),
        );
        Run {
            code,
            out: String::from_utf8(out).unwrap(),
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
fn an_unreadable_lockfile_skips_the_repo_naming_the_file_below_the_project() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", "not json");
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    let before = p.read("refs.toml");

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(
        run.err.contains("cannot read package-lock.json: "),
        "{}",
        run.err
    );
    assert_eq!(p.read("refs.toml"), before);
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
fn a_package_the_lockfile_lacks_or_no_lockfile_moves_to_the_newest_tag_of_its_kind() {
    for lockfile in [Some(NPM_LOCK_WITHOUT_FOO), None] {
        let p = Project::new(&foo_at("foo-js-1.0.0"));
        if let Some(lockfile) = lockfile {
            p.write("package-lock.json", lockfile);
        }
        with_tags(
            &p,
            &[
                "foo-js-1.0.0",
                "foo-js-1.4.0",
                "foo-js-2.0.0-rc.1",
                "foo-js-dom-9.0.0",
            ],
        );

        let run = p.update(&["foo"]);

        assert_eq!(run.code, 0, "{}", run.err);
        assert!(
            run.err.contains("foo  foo-js-1.0.0 → foo-js-1.4.0"),
            "{}",
            run.err
        );
        assert!(
            p.read("refs.toml")
                .contains("ref = \"foo-js-1.4.0\" # pinned")
        );
    }
}

#[test]
fn a_repo_already_at_the_newest_tag_of_its_kind_stays() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    with_tags(&p, &["foo-js-1.0.0", "foo-js-dom-9.0.0"]);
    let before = p.read("refs.toml");

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("`foo` is up to date"), "{}", run.err);
    assert_eq!(p.read("refs.toml"), before);
}

#[test]
fn the_lockfile_decides_even_when_a_newer_tag_exists() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3", "foo-js-2.0.0"]);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains("foo  foo-js-1.0.0 → foo-js-1.2.3"),
        "{}",
        run.err
    );
}

#[test]
fn a_pinned_repo_with_no_origin_moves_to_the_newest_tag_of_its_kind() {
    let config = foo_at("v1.0.0").replace("origin = \"npm:foo-js\"\n", "");
    let p = Project::new(&config);
    with_tags(&p, &["v1.0.0", "v1.1.0", "v2.0.0-rc.1"]);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("foo  v1.0.0 → v1.1.0"), "{}", run.err);
    assert!(p.read("refs.toml").contains("ref = \"v1.1.0\" # pinned"));
}

#[test]
fn list_shows_the_move_to_the_newest_tag_of_its_kind() {
    let config = foo_at("v1.0.0").replace("origin = \"npm:foo-js\"\n", "");
    let p = Project::new(&config);
    with_tags(&p, &["v1.0.0", "v1.1.0"]);

    let run = p.update(&["--list"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.out, "foo  v1.0.0 → v1.1.0\n");
    assert_eq!(p.read("refs.toml"), config);
}

#[test]
fn a_tag_with_no_version_stays_with_a_note_pointing_at_ref() {
    let config = foo_at("stable").replace("origin = \"npm:foo-js\"\n", "");
    let p = Project::new(&config);
    with_tags(&p, &["stable", "v1.0.0"]);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains(
            "`foo` is not moved: `stable` has no version to compare; use `--ref` to move it"
        ),
        "{}",
        run.err
    );
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
    let why = "package-lock.json has several versions of `foo-js` (1.2.3, 1.10.0), and none is \
               the one the project asks for; use `refs update foo --ref <tag>` to choose one";
    assert!(skipped.err.contains(why), "{}", skipped.err);

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
    p.source.set_commit("foo", SHA_2);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(p.read("refs.toml"), BRANCH);
    assert!(p.read("refs.lock").contains(SHA_2));
    assert!(
        run.err.contains("foo  aaaaaaa → 2222222 (main)"),
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

#[test]
fn a_dirty_floating_mover_is_skipped_and_the_lock_stays() {
    let p = Project::new(BRANCH);
    make_dirty(&p, "foo");
    p.source.set_commit("foo", SHA_2);
    let before = p.read("refs.lock");

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("notes.txt"), "{}", run.err);
    assert_eq!(p.read("refs.lock"), before);
}

#[test]
fn a_dirty_repo_is_skipped_before_the_version_question_is_asked() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_TWO);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3", "foo-js-1.10.0"]);
    make_dirty(&p, "foo");
    let mut prompter = Script::new([]);

    let run = p.run(&["update", "foo"], Some(&mut prompter));

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("notes.txt"), "{}", run.err);
    assert!(prompter.asked.is_empty(), "{:?}", prompter.asked);
}

#[test]
fn a_dirty_repo_that_would_not_move_is_not_a_skip() {
    let p = Project::new(&foo_at("foo-js-1.2.3"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.2.3"]);
    make_dirty(&p, "foo");

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("`foo` is up to date"), "{}", run.err);
}

#[test]
fn a_checkout_that_is_not_there_yet_does_not_hold_a_mover_back() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.source.seed("foo", Observed::Absent);

    let run = p.update(&["foo", "--ref", "foo-js-2.0.0"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(p.read("refs.toml").contains("foo-js-2.0.0"));
}

#[test]
fn a_checkout_that_cannot_be_inspected_holds_a_mover_but_not_a_stayer() {
    let p = Project::new(&foo_at("foo-js-1.2.3"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.2.3"]);
    p.source.fail("foo", Method::Inspect, "unreadable");

    let stays = p.update(&["foo"]);
    assert_eq!(stays.code, 0, "{}", stays.err);
    assert!(stays.err.contains("`foo` is up to date"), "{}", stays.err);

    let moves = p.update(&["foo", "--ref", "foo-js-2.0.0"]);
    assert_eq!(moves.code, 1, "{}", moves.err);
    assert!(moves.err.contains("unreadable"), "{}", moves.err);
}

#[test]
fn a_ref_the_remote_no_longer_has_is_skipped() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.source.set_tags(FOO, ["foo-js-2.0.0"]);
    p.source.fail("foo", Method::Resolve, "no such ref");
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("no such ref"), "{}", run.err);
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn a_lock_only_move_that_stage_two_refuses_writes_nothing_and_says_to_sync_once_fixed() {
    let p = Project::new(THREE);
    p.source.set_commit("bar", SHA_2);
    p.source.seed("foo", Observed::Foreign);
    let before = p.read("refs.lock");

    let run = p.update(&["bar"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(
        run.err
            .contains("run `refs sync` once the problem is fixed"),
        "{}",
        run.err
    );
    assert_eq!(p.read("refs.lock"), before);
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

    let run = p.update(&["foo", "bar", "baz"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert_eq!(
        p.read("refs.toml"),
        "[repos.foo]
url = \"https://github.com/o/foo\"
ref = \"foo-js-1.2.3\"
origin = \"npm:foo-js\"

[repos.bar]
url = \"https://github.com/o/bar\"
ref = \"main\"

[repos.baz]
url = \"https://github.com/o/baz\"
ref = \"v1\"
origin = \"npm:baz\"
"
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
}

#[test]
fn a_skip_ends_the_run_with_the_summary_and_not_the_fix_or_remove_hint() {
    let p = Project::new(THREE);
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    p.source.fail(BAZ, Method::Tags, "remote down");
    p.source.fail("foo", Method::Materialise, "disk full");

    let run = p.update(&["foo", "baz"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("disk full"), "{}", run.err);
    assert!(run.err.contains("1 repo was not updated"), "{}", run.err);
    assert!(!run.err.contains("fix or remove"), "{}", run.err);
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
fn update_has_no_offline_option() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    let calls = p.source.calls().len();

    for args in [
        &["update", "foo", "--offline"][..],
        &["update", "--list", "--offline"],
    ] {
        let run = p.run(args, None);
        assert_eq!(run.code, 2, "{}", run.err);
        assert!(run.err.contains("--offline"), "{}", run.err);
    }
    assert_eq!(p.source.calls().len(), calls);
}

#[test]
fn short_and_long_help_print_the_same() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));

    let short = p.run(&["update", "-h"], None);
    let long = p.run(&["update", "--help"], None);

    assert_eq!((short.code, long.code), (0, 0));
    assert_eq!(short.out, long.out);
}

#[test]
fn bare_update_without_a_terminal_lists_and_says_so() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let run = p.update(&[]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.out, "foo  foo-js-1.0.0 → foo-js-1.2.3\n");
    assert!(
        run.err.contains("listing only; name repos to update"),
        "{}",
        run.err
    );
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
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

// ---- what the new Ref breaks ----

/// `foo` pinned to `foo-js-1.0.0` with docs in two directories and a file to start from.
fn foo_with_docs() -> String {
    format!(
        "[repos.foo]\nurl = \"{FOO}\"\nref = \"foo-js-1.0.0\"\norigin = \"npm:foo-js\"\n\
         paths = [\"docs\", \"old\"]\nstart = [\"docs/intro.md\", \"old/a.md\"]\n"
    )
}

/// The remote now has `docs` and `docs/intro.md` but no `old`, and a tag for the lockfile's version.
fn old_is_gone(p: &Project) {
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    p.source.set_tree(
        FOO,
        SHA_2,
        &[("docs/intro.md", "# intro"), ("docs/more.md", "# more")],
    );
}

#[test]
fn a_move_that_loses_a_path_asks_update_and_n_changes_nothing() {
    let p = Project::new(&foo_with_docs());
    old_is_gone(&p);
    let before = (p.read("refs.toml"), p.read("refs.lock"));
    let mut prompter = Script::new([Answer::Yes(false)]);

    let run = p.run(&["update", "foo"], Some(&mut prompter));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(prompter.questions(), ["Update?"]);
    assert!(run.err.contains("paths `old`"), "{}", run.err);
    assert!(run.err.contains("start `old/a.md`"), "{}", run.err);
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn yes_asks_only_the_broken_fields_with_the_valid_entries_ticked() {
    let p = Project::new(&foo_with_docs());
    old_is_gone(&p);
    let mut prompter = Script::new([
        Answer::Yes(true),
        Answer::Pick(&["docs"]),
        Answer::Pick(&["docs/intro.md"]),
    ]);

    let run = p.run(&["update", "foo"], Some(&mut prompter));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(prompter.questions().len(), 3, "{:?}", prompter.questions());
    assert_eq!(prompter.preselected, [["docs"], ["docs/intro.md"]]);
    assert!(prompter.offered[0].contains(&"old".to_owned()));
    assert!(prompter.offered[1].contains(&"old/a.md".to_owned()));
    assert_eq!(
        p.read("refs.toml"),
        format!(
            "[repos.foo]\nurl = \"{FOO}\"\nref = \"foo-js-1.2.3\"\norigin = \"npm:foo-js\"\n\
             paths = [\"docs\"]\nstart = [\"docs/intro.md\"]\n"
        )
    );
    assert!(p.read("refs.lock").contains("ref = \"foo-js-1.2.3\""));
}

#[test]
fn a_move_that_loses_nothing_asks_nothing() {
    let p = Project::new(&foo_with_docs());
    old_is_gone(&p);
    p.source.set_tree(
        FOO,
        SHA_2,
        &[("docs/intro.md", "#"), ("old/a.md", "#"), ("old/b.md", "#")],
    );
    let mut prompter = Script::new([]);

    let run = p.run(&["update", "foo"], Some(&mut prompter));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(prompter.questions().is_empty());
    assert!(p.read("refs.toml").contains("ref = \"foo-js-1.2.3\""));
}

#[test]
fn a_vanished_package_is_kept_and_warned_about() {
    let config = format!(
        "[repos.foo]\nurl = \"{FOO}\"\nref = \"foo-js-1.0.0\"\norigin = \"npm:foo-js\"\n\
         paths = [\"docs\", \"old\"]\npackages = [\"foo-js\", \"left-pad\"]\n"
    );
    let p = Project::new(&config);
    old_is_gone(&p);
    p.source.set_tree(
        FOO,
        SHA_2,
        &[
            ("docs/a.md", "#"),
            ("docs/package.json", r#"{"name":"foo-js"}"#),
        ],
    );
    let mut prompter = Script::new([
        Answer::Yes(true),
        Answer::Pick(&["docs"]),
        Answer::Pick(&["foo-js", "left-pad"]),
    ]);

    let run = p.run(&["update", "foo"], Some(&mut prompter));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("`left-pad`"), "{}", run.err);
    assert_eq!(prompter.preselected[1], ["foo-js", "left-pad"]);
    assert!(
        p.read("refs.toml")
            .contains("packages = [\"foo-js\", \"left-pad\"]")
    );
}

/// The new tree has `docs/intro.md` and `count` packages under `docs`.
fn many_manifests(p: &Project, count: usize) {
    let names: Vec<(String, String)> = (0..count)
        .map(|i| {
            (
                format!("docs/p{i}/package.json"),
                format!(r#"{{"name":"n{i}"}}"#),
            )
        })
        .collect();
    let mut files: Vec<(&str, &str)> = names.iter().map(|(p, t)| (&**p, &**t)).collect();
    files.push(("docs/intro.md", "# intro"));
    p.source.set_tree(FOO, SHA_2, &files);
}

#[test]
fn manifests_beyond_the_cap_are_counted_when_a_break_is_asked() {
    let p = Project::new(&foo_with_docs());
    old_is_gone(&p);
    many_manifests(&p, 230);
    let mut prompter = Script::new([Answer::Yes(false)]);

    let run = p.run(&["update", "foo"], Some(&mut prompter));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.err.contains("read 200 Manifests and left 30 unread"),
        "{}",
        run.err
    );
}

#[test]
fn without_a_terminal_a_break_skips_that_repo_and_writes_nothing() {
    let p = Project::new(&foo_with_docs());
    old_is_gone(&p);
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("`old`"), "{}", run.err);
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn cancelling_update_edits_nothing() {
    let p = Project::new(&foo_with_docs());
    old_is_gone(&p);
    let before = (p.read("refs.toml"), p.read("refs.lock"));
    let mut prompter = Script::new([Answer::Yes(true), Answer::Cancel]);

    let run = p.run(&["update", "foo"], Some(&mut prompter));

    assert_ne!(run.code, 0);
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn a_deleted_ref_is_asked_again_in_a_terminal_and_an_error_without_one() {
    let p = Project::new(&format!("[repos.foo]\nurl = \"{FOO}\"\nref = \"gone\"\n"));
    p.source.lose_ref(FOO, "gone");
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let without = p.update(&["foo"]);
    assert_eq!(without.code, 1, "{}", without.err);
    assert!(without.err.contains("--ref"), "{}", without.err);
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);

    let mut prompter = Script::new([Answer::Text("main")]);
    let asked = p.run(&["update", "foo"], Some(&mut prompter));
    assert_eq!(asked.code, 0, "{}", asked.err);
    assert_eq!(
        prompter.asked,
        [("Ref".to_owned(), Some("gone".to_owned()))]
    );
    assert!(p.read("refs.toml").contains("ref = \"main\""));
}

// ---- bare `update`: the picker ----

#[test]
fn bare_update_offers_every_repo_with_nothing_ticked_and_moves_the_picked_one() {
    let p = Project::new(THREE);
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    p.source.fail(BAZ, Method::Tags, "remote down");
    let mut prompter = Script::new([Answer::Pick(&["foo  foo-js-1.0.0 → foo-js-1.2.3"])]);

    let run = p.run(&["update"], Some(&mut prompter));

    assert_eq!(run.code, 0, "{}", run.err);
    let offered = &prompter.offered[0];
    assert_eq!(offered.len(), 3, "{offered:?}");
    assert_eq!(offered[0], "foo  foo-js-1.0.0 → foo-js-1.2.3");
    assert_eq!(offered[1], "bar  up to date");
    assert_eq!(offered[2], "baz  (cannot check: remote down)");
    assert!(prompter.preselected[0].is_empty());
    assert!(run.err.contains("checking 3 repos…"), "{}", run.err);
    let toml = p.read("refs.toml");
    assert!(toml.contains("ref = \"foo-js-1.2.3\""), "{toml}");
    assert!(toml.contains("ref = \"v1\""), "{toml}");
}

#[test]
fn picking_a_repo_that_could_not_be_checked_skips_it() {
    let p = Project::new(THREE);
    p.source.fail(BAZ, Method::Tags, "remote down");
    let before = p.read("refs.toml");
    let mut prompter = Script::new([Answer::Pick(&["baz  (cannot check: remote down)"])]);

    let run = p.run(&["update"], Some(&mut prompter));

    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("1 repo was not updated"), "{}", run.err);
    assert_eq!(p.read("refs.toml"), before);
}

#[test]
fn an_ambiguous_lockfile_version_is_labelled_and_asked_only_when_picked() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_TWO);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3", "foo-js-1.10.0"]);

    let mut left = Script::new([Answer::Pick(&[])]);
    let run = p.run(&["update"], Some(&mut left));
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(left.offered[0], ["foo  foo-js-1.0.0 → choose version…"]);
    assert_eq!(left.questions().len(), 1, "{:?}", left.questions());

    let mut picked = Script::new([
        Answer::Pick(&["foo  foo-js-1.0.0 → choose version…"]),
        Answer::Text("1.2.3"),
    ]);
    let run = p.run(&["update"], Some(&mut picked));
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(p.read("refs.toml").contains("ref = \"foo-js-1.2.3\""));
}

#[test]
fn a_dirty_repo_is_labelled_and_picking_it_skips_it_with_the_files_named() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    make_dirty(&p, "foo");
    let before = p.read("refs.toml");
    const LABEL: &str = "foo  foo-js-1.0.0 → foo-js-1.2.3  (uncommitted changes)";
    let mut prompter = Script::new([Answer::Pick(&[LABEL])]);

    let run = p.run(&["update"], Some(&mut prompter));

    assert_eq!(prompter.offered[0], [LABEL]);
    assert_eq!(run.code, 1, "{}", run.err);
    assert!(run.err.contains("notes.txt"), "{}", run.err);
    assert_eq!(p.read("refs.toml"), before);
}

#[test]
fn picking_nothing_or_cancelling_edits_nothing() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let mut none = Script::new([Answer::Pick(&[])]);
    assert_eq!(p.run(&["update"], Some(&mut none)).code, 0);
    let mut cancelled = Script::new([Answer::Cancel]);
    assert_ne!(p.run(&["update"], Some(&mut cancelled)).code, 0);

    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn naming_ids_skips_the_picker() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    let mut prompter = Script::new([]);

    let run = p.run(&["update", "foo"], Some(&mut prompter));

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(prompter.offered.is_empty());
}

// ---- --list ----

#[test]
fn list_prints_what_would_move_on_stdout_and_changes_nothing() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let bare = p.update(&["--list"]);
    let mut prompter = Script::new([]);
    let in_terminal = p.run(&["update", "--list"], Some(&mut prompter));

    for run in [&bare, &in_terminal] {
        assert_eq!(run.code, 0, "{}", run.err);
        assert_eq!(run.out, "foo  foo-js-1.0.0 → foo-js-1.2.3\n");
        assert!(run.err.contains("checking 1 repos…"), "{}", run.err);
        assert!(!run.err.contains("listing only"), "{}", run.err);
        assert!(!run.err.contains("→"), "{}", run.err);
    }
    assert!(prompter.questions().is_empty());
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn naming_a_repo_does_not_announce_a_check_of_all() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);

    let run = p.update(&["foo"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(!run.err.contains("checking"), "{}", run.err);
}

#[test]
fn list_says_what_a_mover_would_break() {
    let p = Project::new(&foo_with_docs());
    old_is_gone(&p);
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let run = p.update(&["--list"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        run.out,
        "foo  foo-js-1.0.0 → foo-js-1.2.3\n  no longer has paths `old`; start `old/a.md`\n"
    );
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn list_marks_a_dirty_mover_with_its_files_and_still_shows_its_breaks() {
    let p = Project::new(&foo_with_docs());
    old_is_gone(&p);
    make_dirty(&p, "foo");
    let before = (p.read("refs.toml"), p.read("refs.lock"));

    let run = p.update(&["--list"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        run.out,
        "foo  foo-js-1.0.0 → foo-js-1.2.3\n  no longer has paths `old`; start `old/a.md`\n  \
         would be skipped: uncommitted changes (notes.txt)\n"
    );
    assert_eq!((p.read("refs.toml"), p.read("refs.lock")), before);
}

#[test]
fn list_shows_an_ambiguous_version_and_never_asks() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_TWO);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3", "foo-js-1.10.0"]);
    let mut prompter = Script::new([]);

    let run = p.run(&["update", "--list"], Some(&mut prompter));

    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        run.out,
        "foo  foo-js-1.0.0 → choose version…\n  package-lock.json has 1.2.3, 1.10.0\n"
    );
    assert!(prompter.questions().is_empty());
}

#[test]
fn list_with_ids_lists_only_those_and_offline_is_an_error() {
    let p = Project::new(&format!(
        "{}\n[repos.bar]\nurl = \"{BAR}\"\nref = \"v1\"\n",
        foo_at("foo-js-1.0.0")
    ));
    p.source.set_tags(BAR, ["v1"]);

    let run = p.update(&["--list", "bar"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        run.out,
        "bar  not moved: `v1` has no version to compare; use `--ref` to move it\n"
    );
}

#[test]
fn list_marks_a_mover_whose_checkout_cannot_be_inspected() {
    let p = Project::new(&foo_at("foo-js-1.0.0"));
    p.write("package-lock.json", NPM_LOCK_ONE);
    with_tags(&p, &["foo-js-1.0.0", "foo-js-1.2.3"]);
    p.source.fail("foo", Method::Inspect, "unreadable");

    let run = p.update(&["--list"]);

    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.out
            .starts_with("foo  foo-js-1.0.0 → foo-js-1.2.3\n  would be skipped: "),
        "{}",
        run.out
    );
    assert!(run.out.contains("unreadable"), "{}", run.out);
}
