//! What `add` pins a registry package to: the version given or found in a Package lockfile,
//! and the tag it was released as, with what is said and asked on the way. Over temp
//! directories, a scripted tag list and scripted answers.

use std::fs;

use refs_cli::add::pin::{Pinned, pin};
use refs_cli::failure::Failure;
use refs_cli::registry::{Ecosystem, Package};
use refs_cli::source::fake::{FakeSource, Method};
use refs_cli::ui::{Abort, Say};
use tempfile::TempDir;

mod common;
use common::script::Script;

const URL: &str = "https://github.com/o/serde";

const ONE_SERDE: &str = r#"
version = 4

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

const TWO_SERDES: &str = r#"
version = 4

[[package]]
name = "serde"
version = "1.0.100"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

const HINT: &str = "use `--ref` to track a branch or choose another tag";
const NOT_PINNED: &str =
    "no version is pinned; the Ref is the remote's default branch unless one is given";
const FOLLOWS: &str = "the repo follows the remote's default branch";

/// What was said, in order: `status: ...` or `warning: ...`.
#[derive(Default)]
struct Lines(Vec<String>);

impl Say for Lines {
    fn status(&mut self, line: String) {
        self.0.push(format!("status: {line}"));
    }
    fn warning(&mut self, line: String) {
        self.0.push(format!("warning: {line}"));
    }
    fn shown(&mut self, line: String) {
        self.0.push(format!("shown: {line}"));
    }
}

fn serde() -> Package {
    Package::new(Ecosystem::Cargo, "serde")
}

fn project(lock: Option<&str>) -> TempDir {
    let dir = TempDir::new().unwrap();
    if let Some(lock) = lock {
        fs::write(dir.path().join("Cargo.lock"), lock).unwrap();
    }
    dir
}

fn tagged(names: &[&str]) -> FakeSource {
    let source = FakeSource::new();
    source.set_tags(URL, names.iter().copied());
    source
}

fn unreachable() -> FakeSource {
    let source = FakeSource::new();
    source.fail(URL, Method::Tags, "no route");
    source
}

/// Pin `serde` at `version` in a project with `lock`, listing `source`'s tags, with no one to ask.
fn run(
    version: Option<&str>,
    dir: &TempDir,
    source: Option<&FakeSource>,
) -> (Result<Pinned, Failure>, Vec<String>) {
    let mut say = Lines::default();
    let source = source.map(|s| s as &dyn refs_cli::source::Source);
    let pinned = pin(&serde(), version, URL, dir.path(), source, None, &mut say);
    (pinned, say.0)
}

#[test]
fn a_version_given_is_pinned_to_its_tag_without_reading_a_lockfile() {
    let dir = project(Some(TWO_SERDES));
    let (pinned, said) = run(Some("1.0.150"), &dir, Some(&tagged(&["v1.0.150"])));
    assert_eq!(pinned.unwrap(), Pinned::Tag("v1.0.150".into()));
    assert_eq!(
        said,
        [format!(
            "status: pinned `serde@1.0.150` to the tag `v1.0.150`; {HINT}"
        )]
    );
}

#[test]
fn a_version_with_no_tag_follows_the_default_branch_with_a_warning() {
    let dir = project(None);
    let (pinned, said) = run(Some("1.0.150"), &dir, Some(&tagged(&["v1.0.149"])));
    assert_eq!(pinned.unwrap(), Pinned::FollowsDefault);
    assert_eq!(
        said,
        [format!(
            "warning: serde@1.0.150 is not tagged in {URL}; {FOLLOWS}; {HINT}"
        )]
    );
}

#[test]
fn a_person_is_asked_before_an_untagged_version_follows_the_default_branch() {
    let dir = project(None);
    let source = tagged(&[]);
    let mut say = Lines::default();
    let mut yes = Script::takes_defaults(true);
    let pinned = pin(
        &serde(),
        Some("1.0.150"),
        URL,
        dir.path(),
        Some(&source),
        Some(&mut yes),
        &mut say,
    );
    assert_eq!(pinned.unwrap(), Pinned::FollowsDefault);
    assert_eq!(
        yes.questions(),
        [format!(
            "serde@1.0.150 is not tagged in {URL}. Follow the remote's default branch instead?"
        )]
    );
    assert_eq!(say.0, [format!("status: {FOLLOWS}; {HINT}")]);

    let mut no = Script::takes_defaults(false);
    let pinned = pin(
        &serde(),
        Some("1.0.150"),
        URL,
        dir.path(),
        Some(&source),
        Some(&mut no),
        &mut say,
    );
    assert!(matches!(pinned, Err(Failure::Asked(Abort::Cancelled))));
}

#[test]
fn without_tags_to_list_a_version_is_not_pinned() {
    let dir = project(None);
    let (pinned, said) = run(Some("1.0.150"), &dir, None);
    assert_eq!(pinned.unwrap(), Pinned::FollowsDefault);
    assert_eq!(
        said,
        [format!(
            "warning: `serde@1.0.150` is not pinned to a tag, as `--no-sync` lists no tags; {FOLLOWS}; {HINT}"
        )]
    );
}

#[test]
fn no_lockfile_follows_the_default_branch() {
    let dir = project(None);
    let (pinned, said) = run(None, &dir, Some(&tagged(&["v1.0.200"])));
    assert_eq!(pinned.unwrap(), Pinned::Unpinned);
    assert_eq!(
        said,
        [format!(
            "status: no Package lockfile found for `serde`; {NOT_PINNED}"
        )]
    );
}

#[test]
fn the_used_version_is_pinned_to_its_tag_and_the_file_read_is_named() {
    let dir = project(Some(ONE_SERDE));
    let (pinned, said) = run(None, &dir, Some(&tagged(&["v1.0.199", "v1.0.200"])));
    assert_eq!(pinned.unwrap(), Pinned::Tag("v1.0.200".into()));
    assert_eq!(
        said,
        [
            "status: `serde` is 1.0.200 in Cargo.lock".to_owned(),
            format!("status: pinned `serde@1.0.200` to the tag `v1.0.200`; {HINT}")
        ]
    );
}

#[test]
fn a_used_version_with_no_tag_still_names_what_was_read() {
    let dir = project(Some(ONE_SERDE));
    let (pinned, said) = run(None, &dir, Some(&tagged(&[])));
    assert_eq!(pinned.unwrap(), Pinned::FollowsDefault);
    assert_eq!(said[0], "status: `serde` is 1.0.200 in Cargo.lock");
    assert!(said[1].starts_with("warning: serde@1.0.200 is not tagged in"));
}

#[test]
fn a_package_the_lockfile_lacks_follows_the_default_branch() {
    let dir = project(Some(ONE_SERDE));
    let other = Package::new(Ecosystem::Cargo, "tokio");
    let mut say = Lines::default();
    let source = tagged(&[]);
    let pinned = pin(&other, None, URL, dir.path(), Some(&source), None, &mut say);
    assert_eq!(pinned.unwrap(), Pinned::Unpinned);
    assert_eq!(
        say.0,
        [format!(
            "status: `tokio` is not in Cargo.lock; {NOT_PINNED}"
        )]
    );
}

#[test]
fn several_versions_are_asked_and_the_one_chosen_is_pinned() {
    let dir = project(Some(TWO_SERDES));
    let source = tagged(&["v1.0.100", "v1.0.200"]);
    let mut say = Lines::default();
    let mut picks = Script::takes_defaults(true).picking("versions", Some("1.0.100"));
    let pinned = pin(
        &serde(),
        None,
        URL,
        dir.path(),
        Some(&source),
        Some(&mut picks),
        &mut say,
    );
    assert_eq!(pinned.unwrap(), Pinned::Tag("v1.0.100".into()));
    assert_eq!(
        picks.questions(),
        ["Cargo.lock has several versions of `serde`: 1.0.100, 1.0.200. Which one?"]
    );
}

#[test]
fn several_versions_with_no_one_to_ask_are_refused() {
    let dir = project(Some(TWO_SERDES));
    let (pinned, _) = run(None, &dir, Some(&tagged(&["v1.0.200"])));
    let Err(Failure::Refused(why)) = pinned else {
        panic!("expected a refusal");
    };
    assert_eq!(
        why,
        "Cargo.lock has several versions of `serde` (1.0.100, 1.0.200), and none is the one \
         the project asks for; give one as `serde@version`"
    );
}

#[test]
fn an_unreadable_lockfile_follows_the_default_branch_and_says_why() {
    let dir = project(Some("this is not toml ["));
    let (pinned, said) = run(None, &dir, Some(&tagged(&[])));
    assert_eq!(pinned.unwrap(), Pinned::Unpinned);
    assert!(
        said[0].starts_with("warning: cannot read Cargo.lock: "),
        "{said:?}"
    );
    assert!(said[0].ends_with(NOT_PINNED));
}

#[test]
fn another_lockfile_of_the_ecosystem_in_the_same_directory_is_named_as_not_read() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    fs::write(dir.path().join("yarn.lock"), "# yarn lockfile v1\n").unwrap();
    let react = Package::new(Ecosystem::Npm, "react");
    let mut say = Lines::default();
    pin(&react, None, URL, dir.path(), None, None, &mut say).unwrap();
    assert_eq!(
        say.0[0],
        "status: yarn.lock is also here and was not read; pnpm-lock.yaml was"
    );
}

#[test]
fn failing_to_list_tags_still_names_what_was_read_first() {
    let dir = project(Some(ONE_SERDE));
    let (pinned, said) = run(None, &dir, Some(&unreachable()));
    assert!(matches!(pinned, Err(Failure::Source(_))));
    assert_eq!(said, ["status: `serde` is 1.0.200 in Cargo.lock"]);
}

#[test]
fn failing_to_list_tags_for_a_given_version_read_nothing() {
    let dir = project(Some(ONE_SERDE));
    let (pinned, said) = run(Some("1.0.1"), &dir, Some(&unreachable()));
    assert!(matches!(pinned, Err(Failure::Source(_))));
    assert!(said.is_empty());
}
