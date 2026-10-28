//! Which Package lockfile a project's `add` reads, over real directories.

use std::fs;
use std::path::{Path, PathBuf};

mod common;

use refs_cli::registry::used::{Answer, answer};
use refs_cli::registry::{Ecosystem, Package};
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

const UV_LOCK: &str = r#"
version = 1

[[package]]
name = "requests"
version = "2.32.0"
source = { registry = "https://pypi.org/simple" }
"#;

const BUN_LOCK: &str = r#"{"lockfileVersion":1,
  "workspaces":{"":{"name":"app","dependencies":{"react":"^18",},},},
  "packages":{"react":["react@18.3.1","",{},"sha512-x"],},}"#;

const POETRY_LOCK: &str = "[[package]]\nname = \"requests\"\nversion = \"2.31.0\"\n";

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap()
}

/// The file `answer` read, as `answer` names it, and its versions.
fn read(answer: &Answer) -> (&str, Vec<&str>) {
    match answer {
        Answer::Absent { file, .. } => (file, vec![]),
        Answer::One { file, version, .. } => (file, vec![version]),
        Answer::Several { file, versions, .. } => {
            (file, versions.iter().map(String::as_str).collect())
        }
        other => panic!("expected a lockfile, got {other:?}"),
    }
}

fn ignored(answer: &Answer) -> &[String] {
    match answer {
        Answer::Absent { ignored, .. }
        | Answer::One { ignored, .. }
        | Answer::Several { ignored, .. } => ignored,
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

fn ask(ecosystem: Ecosystem, name: &str, project: &Path) -> Answer {
    answer(&Package::new(ecosystem, name), project)
}

#[test]
fn a_lockfile_further_up_is_found_as_far_as_the_worktree_top() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("Cargo.lock"), CARGO_LOCK).unwrap();

    let found = ask(Ecosystem::Cargo, "serde", &project);

    assert_eq!(
        read(&found),
        (top.join("Cargo.lock").to_str().unwrap(), vec!["1.0.200"])
    );
}

#[test]
fn outside_a_worktree_only_the_project_directory_is_read() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("Cargo.lock"), CARGO_LOCK).unwrap();
    let project = dir.path().join("sub");
    fs::create_dir_all(&project).unwrap();

    assert_eq!(
        ask(Ecosystem::Cargo, "serde", &canonical(&project)),
        Answer::Missing
    );
    let found = ask(Ecosystem::Cargo, "serde", &canonical(dir.path()));
    assert_eq!(read(&found), ("Cargo.lock", vec!["1.0.200"]));
}

#[test]
fn the_nearest_lockfile_wins_even_when_it_lacks_the_package() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("Cargo.lock"), CARGO_LOCK).unwrap();
    fs::write(project.join("Cargo.lock"), "version = 4\n").unwrap();

    let found = ask(Ecosystem::Cargo, "serde", &project);

    assert_eq!(read(&found), ("Cargo.lock", vec![]));
}

#[test]
fn of_uv_lock_and_poetry_lock_in_one_directory_uv_lock_is_read() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("poetry.lock"), POETRY_LOCK).unwrap();
    fs::write(project.join("uv.lock"), UV_LOCK).unwrap();

    let found = ask(Ecosystem::Pypi, "requests", &project);

    assert_eq!(read(&found), ("uv.lock", vec!["2.32.0"]));
    assert_eq!(ignored(&found), ["poetry.lock"]);
}

#[test]
fn the_binary_bun_lockb_is_never_read() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("bun.lockb"), b"#!/usr/bin/env bun\n\0\0").unwrap();

    assert_eq!(ask(Ecosystem::Npm, "react", &project), Answer::Missing);
}

#[test]
fn a_lockfile_that_is_not_its_format_is_unreadable() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("Cargo.lock"), "{ not toml").unwrap();

    assert!(matches!(
        ask(Ecosystem::Cargo, "serde", &project),
        Answer::Unreadable { file, .. } if file == "Cargo.lock"
    ));
}

#[test]
fn of_several_npm_lockfiles_in_one_directory_pnpm_then_yarn_then_npm_then_bun_is_read() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("package-lock.json"), PACKAGE_LOCK).unwrap();
    fs::write(project.join("yarn.lock"), YARN_LOCK).unwrap();
    let found = ask(Ecosystem::Npm, "react", &project);
    assert_eq!(read(&found), ("yarn.lock", vec!["18.1.0"]));
    assert_eq!(ignored(&found), ["package-lock.json"]);

    fs::write(project.join("pnpm-lock.yaml"), PNPM_LOCK).unwrap();
    let found = ask(Ecosystem::Npm, "react", &project);
    assert_eq!(read(&found), ("pnpm-lock.yaml", vec!["18.3.1"]));
    assert_eq!(ignored(&found), ["yarn.lock", "package-lock.json"]);

    fs::write(project.join("bun.lock"), BUN_LOCK).unwrap();
    let found = ask(Ecosystem::Npm, "react", &project);
    assert_eq!(
        ignored(&found),
        ["yarn.lock", "package-lock.json", "bun.lock"]
    );
}

#[test]
fn of_several_versions_only_the_one_the_project_asks_for_is_picked() {
    let (_repo, _top, project) = worktree();
    let lock = r#"{"lockfileVersion":3,"packages":{
      "": {"dependencies":{"ms":"^2.1"}},
      "node_modules/ms":{"version":"2.1.3","resolved":"https://registry.npmjs.org/ms/-/ms-2.1.3.tgz"},
      "node_modules/x/node_modules/ms":{"version":"2.0.0","resolved":"https://registry.npmjs.org/ms/-/ms-2.0.0.tgz"}}}"#;
    fs::write(project.join("package-lock.json"), lock).unwrap();

    assert_eq!(
        read(&ask(Ecosystem::Npm, "ms", &project)),
        ("package-lock.json", vec!["2.1.3"])
    );
    let lock = lock.replace(r#""dependencies":{"ms":"^2.1"}"#, r#""dependencies":{}"#);
    fs::write(project.join("package-lock.json"), lock).unwrap();
    assert_eq!(
        read(&ask(Ecosystem::Npm, "ms", &project)),
        ("package-lock.json", vec!["2.0.0", "2.1.3"])
    );
}

#[test]
fn poetry_lock_is_read_when_it_is_the_only_one() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("poetry.lock"), POETRY_LOCK).unwrap();

    let found = ask(Ecosystem::Pypi, "requests", &project);

    assert_eq!(read(&found), ("poetry.lock", vec!["2.31.0"]));
}
