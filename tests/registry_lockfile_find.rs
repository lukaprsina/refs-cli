//! Which Package lockfile a project's `add` reads, over real directories.

use std::fs;
use std::path::{Path, PathBuf};

mod common;

use refs_cli::registry::Ecosystem;
use refs_cli::registry::lockfile::{Format, Lookup, find};
use tempfile::TempDir;

const CARGO_LOCK: &str = r#"
version = 4

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

const PACKAGE_LOCK: &str = r#"{"lockfileVersion":3,"packages":{
  "": {"dependencies":{"react":"^18"}},
  "node_modules/react":{"version":"18.2.0","resolved":"https://registry.npmjs.org/react/-/react-18.2.0.tgz"}}}"#;

const PNPM_LOCK: &str = "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      react:\n        specifier: ^18\n        version: 18.3.1\n";

const YARN_LOCK: &str = "# yarn lockfile v1\n\n\"react@^18\":\n  version \"18.1.0\"\n";

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap()
}

fn versions(lookup: &Lookup) -> Vec<&str> {
    match lookup {
        Lookup::Found(found) => found.used.iter().map(|u| u.version.as_str()).collect(),
        other => panic!("expected a lockfile, got {other:?}"),
    }
}

fn found_file(lookup: &Lookup) -> PathBuf {
    match lookup {
        Lookup::Found(found) => found.file.clone(),
        other => panic!("expected a lockfile, got {other:?}"),
    }
}

/// A git worktree with `apps/web` in it, and its path.
fn worktree() -> (TempDir, PathBuf, PathBuf) {
    let repo = TempDir::new().unwrap();
    common::git(repo.path(), &["init", "-q"]);
    let top = canonical(repo.path());
    let project = top.join("apps/web");
    fs::create_dir_all(&project).unwrap();
    (repo, top, project)
}

#[test]
fn a_lockfile_in_the_project_directory_is_read() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("Cargo.lock"), CARGO_LOCK).unwrap();

    let lookup = find(Ecosystem::Cargo, "serde", &project);

    assert_eq!(versions(&lookup), ["1.0.200"]);
    assert_eq!(found_file(&lookup), project.join("Cargo.lock"));
}

#[test]
fn a_lockfile_further_up_is_found_as_far_as_the_worktree_top() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("Cargo.lock"), CARGO_LOCK).unwrap();

    assert_eq!(
        found_file(&find(Ecosystem::Cargo, "serde", &project)),
        top.join("Cargo.lock")
    );
}

#[test]
fn a_lockfile_above_the_worktree_is_not_read() {
    let outer = TempDir::new().unwrap();
    fs::write(outer.path().join("Cargo.lock"), CARGO_LOCK).unwrap();
    let repo = outer.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    common::git(&repo, &["init", "-q"]);

    assert!(matches!(
        find(Ecosystem::Cargo, "serde", &canonical(&repo)),
        Lookup::Missing
    ));
}

#[test]
fn outside_a_worktree_only_the_project_directory_is_read() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("Cargo.lock"), CARGO_LOCK).unwrap();
    let project = dir.path().join("sub");
    fs::create_dir_all(&project).unwrap();

    assert!(matches!(
        find(Ecosystem::Cargo, "serde", &canonical(&project)),
        Lookup::Missing
    ));
    assert_eq!(
        versions(&find(Ecosystem::Cargo, "serde", &canonical(dir.path()))),
        ["1.0.200"]
    );
}

#[test]
fn the_nearest_lockfile_wins_even_when_it_lacks_the_package() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("Cargo.lock"), CARGO_LOCK).unwrap();
    fs::write(project.join("Cargo.lock"), "version = 4\n").unwrap();

    let lookup = find(Ecosystem::Cargo, "serde", &project);

    assert_eq!(found_file(&lookup), project.join("Cargo.lock"));
    assert_eq!(versions(&lookup), Vec::<&str>::new());
}

#[test]
fn an_ecosystem_reads_only_its_own_lockfiles() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("package-lock.json"), PACKAGE_LOCK).unwrap();

    assert!(matches!(
        find(Ecosystem::Cargo, "serde", &project),
        Lookup::Missing
    ));
    assert!(matches!(
        find(Ecosystem::Pypi, "requests", &project),
        Lookup::Missing
    ));
    assert_eq!(
        versions(&find(Ecosystem::Npm, "react", &project)),
        ["18.2.0"]
    );
}

#[test]
fn of_several_npm_lockfiles_in_one_directory_pnpm_then_yarn_then_npm_is_read() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("package-lock.json"), PACKAGE_LOCK).unwrap();
    fs::write(project.join("yarn.lock"), YARN_LOCK).unwrap();
    let Lookup::Found(found) = find(Ecosystem::Npm, "react", &project) else {
        panic!("expected a lockfile");
    };
    assert_eq!(found.format, Format::Yarn);
    assert_eq!(found.ignored, [project.join("package-lock.json")]);

    fs::write(project.join("pnpm-lock.yaml"), PNPM_LOCK).unwrap();
    let Lookup::Found(found) = find(Ecosystem::Npm, "react", &project) else {
        panic!("expected a lockfile");
    };
    assert_eq!(found.format, Format::Pnpm);
    assert_eq!(found.used[0].version, "18.3.1");
    assert_eq!(
        found.ignored,
        [project.join("yarn.lock"), project.join("package-lock.json")]
    );
}

#[test]
fn no_lockfile_is_missing() {
    let (_repo, _top, project) = worktree();

    assert!(matches!(
        find(Ecosystem::Npm, "react", &project),
        Lookup::Missing
    ));
}

#[test]
fn a_lockfile_that_cannot_be_read_says_which_and_why() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("Cargo.lock"), "{ not toml").unwrap();

    match find(Ecosystem::Cargo, "serde", &project) {
        Lookup::Unreadable { file, why } => {
            assert_eq!(file, project.join("Cargo.lock"));
            assert!(!why.is_empty());
        }
        other => panic!("expected Unreadable, got {other:?}"),
    }
}
