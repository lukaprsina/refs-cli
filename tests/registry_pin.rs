//! What `add` pins a registry package to: the version given or found in a Package lockfile,
//! and the tag it was released as. Over temp directories and a scripted tag list.

use std::fs;

use refs_cli::registry::pin::{Choice, NoTags, Pinned, Why, pin};
use refs_cli::registry::{Ecosystem, Package};
use refs_cli::source::fake::FakeSource;
use tempfile::TempDir;

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

fn run(version: Option<&str>, dir: &TempDir, source: &FakeSource) -> Pinned {
    pin(&serde(), version, URL, dir.path(), source).unwrap()
}

#[test]
fn a_version_given_is_pinned_to_its_tag_without_reading_a_lockfile() {
    let dir = project(Some(TWO_SERDES));
    let pinned = run(Some("1.0.150"), &dir, &tagged(&["v1.0.150"]));
    assert_eq!(
        pinned.choice,
        Choice::Tag {
            version: "1.0.150".into(),
            tag: "v1.0.150".into()
        }
    );
    assert_eq!(pinned.read, None);
}

#[test]
fn a_version_with_no_tag_follows_the_default_branch() {
    let dir = project(None);
    let pinned = run(Some("1.0.150"), &dir, &tagged(&["v1.0.149"]));
    assert_eq!(
        pinned.choice,
        Choice::DefaultBranch(Why::Untagged {
            version: "1.0.150".into()
        })
    );
}

#[test]
fn without_tags_to_list_a_version_is_not_pinned() {
    let dir = project(None);
    let pinned = pin(&serde(), Some("1.0.150"), URL, dir.path(), &NoTags).unwrap();
    assert_eq!(
        pinned.choice,
        Choice::DefaultBranch(Why::Unlisted {
            version: "1.0.150".into()
        })
    );
}

#[test]
fn no_lockfile_follows_the_default_branch() {
    let dir = project(None);
    let pinned = run(None, &dir, &tagged(&["v1.0.200"]));
    assert_eq!(pinned.choice, Choice::DefaultBranch(Why::NoLockfile));
    assert_eq!(pinned.read, None);
}

#[test]
fn the_used_version_is_pinned_to_its_tag_and_the_file_read_is_named() {
    let dir = project(Some(ONE_SERDE));
    let pinned = run(None, &dir, &tagged(&["v1.0.199", "v1.0.200"]));
    assert_eq!(
        pinned.choice,
        Choice::Tag {
            version: "1.0.200".into(),
            tag: "v1.0.200".into()
        }
    );
    let read = pinned.read.unwrap();
    assert_eq!(read.file, dir.path().join("Cargo.lock"));
    assert_eq!(read.version.as_deref(), Some("1.0.200"));
}

#[test]
fn a_used_version_with_no_tag_still_names_what_was_read() {
    let dir = project(Some(ONE_SERDE));
    let pinned = run(None, &dir, &tagged(&[]));
    assert_eq!(
        pinned.choice,
        Choice::DefaultBranch(Why::Untagged {
            version: "1.0.200".into()
        })
    );
    assert_eq!(pinned.read.unwrap().version.as_deref(), Some("1.0.200"));
}

#[test]
fn a_package_the_lockfile_lacks_follows_the_default_branch() {
    let dir = project(Some(ONE_SERDE));
    let other = Package::new(Ecosystem::Cargo, "tokio");
    let pinned = pin(&other, None, URL, dir.path(), &tagged(&[])).unwrap();
    assert_eq!(
        pinned.choice,
        Choice::DefaultBranch(Why::NotInLockfile {
            file: dir.path().join("Cargo.lock")
        })
    );
    assert_eq!(pinned.read.unwrap().version, None);
}

#[test]
fn several_versions_with_nothing_to_choose_between_them_are_left_to_the_caller() {
    let dir = project(Some(TWO_SERDES));
    let pinned = run(None, &dir, &tagged(&["v1.0.200"]));
    assert_eq!(
        pinned.choice,
        Choice::Several {
            file: dir.path().join("Cargo.lock"),
            versions: vec!["1.0.100".into(), "1.0.200".into()]
        }
    );
}

#[test]
fn an_unreadable_lockfile_follows_the_default_branch_and_says_why() {
    let dir = project(Some("this is not toml ["));
    let pinned = run(None, &dir, &tagged(&[]));
    let Choice::DefaultBranch(Why::Unreadable { file, .. }) = pinned.choice else {
        panic!("expected an unreadable lockfile, got {:?}", pinned.choice);
    };
    assert_eq!(file, dir.path().join("Cargo.lock"));
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
    let pinned = pin(&react, None, URL, dir.path(), &NoTags).unwrap();
    let read = pinned.read.unwrap();
    assert_eq!(read.file, dir.path().join("pnpm-lock.yaml"));
    assert_eq!(read.ignored, [dir.path().join("yarn.lock")]);
}
