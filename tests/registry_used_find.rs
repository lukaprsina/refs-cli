//! Which Package lockfile a project's `add` reads, over real directories.

use std::fs;
use std::path::{Path, PathBuf};

mod common;

use refs_cli::registry::used::{Lookup, find};
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
fn a_lockfile_further_up_is_found_as_far_as_the_worktree_top() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("Cargo.lock"), CARGO_LOCK).unwrap();

    assert_eq!(
        found_file(&find(&Package::new(Ecosystem::Cargo, "serde"), &project)),
        top.join("Cargo.lock")
    );
}

#[test]
fn outside_a_worktree_only_the_project_directory_is_read() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("Cargo.lock"), CARGO_LOCK).unwrap();
    let project = dir.path().join("sub");
    fs::create_dir_all(&project).unwrap();

    assert!(matches!(
        find(
            &Package::new(Ecosystem::Cargo, "serde"),
            &canonical(&project)
        ),
        Lookup::Missing
    ));
    assert_eq!(
        versions(&find(
            &Package::new(Ecosystem::Cargo, "serde"),
            &canonical(dir.path())
        )),
        ["1.0.200"]
    );
}

#[test]
fn the_nearest_lockfile_wins_even_when_it_lacks_the_package() {
    let (_repo, top, project) = worktree();
    fs::write(top.join("Cargo.lock"), CARGO_LOCK).unwrap();
    fs::write(project.join("Cargo.lock"), "version = 4\n").unwrap();

    let lookup = find(&Package::new(Ecosystem::Cargo, "serde"), &project);

    assert_eq!(found_file(&lookup), project.join("Cargo.lock"));
    assert_eq!(versions(&lookup), Vec::<&str>::new());
}

#[test]
fn of_uv_lock_and_poetry_lock_in_one_directory_uv_lock_is_read() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("poetry.lock"), POETRY_LOCK).unwrap();
    fs::write(project.join("uv.lock"), UV_LOCK).unwrap();
    let Lookup::Found(found) = find(&Package::new(Ecosystem::Pypi, "requests"), &project) else {
        panic!("expected a lockfile");
    };
    assert_eq!(found.file, project.join("uv.lock"));
    assert_eq!(found.used[0].version, "2.32.0");
    assert_eq!(found.ignored, [project.join("poetry.lock")]);
}

#[test]
fn the_binary_bun_lockb_is_never_read() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("bun.lockb"), b"#!/usr/bin/env bun\n\0\0").unwrap();

    assert!(matches!(
        find(&Package::new(Ecosystem::Npm, "react"), &project),
        Lookup::Missing
    ));
}

#[test]
fn of_several_npm_lockfiles_in_one_directory_pnpm_then_yarn_then_npm_is_read() {
    let (_repo, _top, project) = worktree();
    fs::write(project.join("package-lock.json"), PACKAGE_LOCK).unwrap();
    fs::write(project.join("yarn.lock"), YARN_LOCK).unwrap();
    let Lookup::Found(found) = find(&Package::new(Ecosystem::Npm, "react"), &project) else {
        panic!("expected a lockfile");
    };
    assert_eq!(found.file, project.join("yarn.lock"));
    assert_eq!(found.ignored, [project.join("package-lock.json")]);

    fs::write(project.join("pnpm-lock.yaml"), PNPM_LOCK).unwrap();
    let Lookup::Found(found) = find(&Package::new(Ecosystem::Npm, "react"), &project) else {
        panic!("expected a lockfile");
    };
    assert_eq!(found.file, project.join("pnpm-lock.yaml"));
    assert_eq!(found.used[0].version, "18.3.1");
    assert_eq!(
        found.ignored,
        [project.join("yarn.lock"), project.join("package-lock.json")]
    );

    fs::write(project.join("bun.lock"), BUN_LOCK).unwrap();
    let Lookup::Found(found) = find(&Package::new(Ecosystem::Npm, "react"), &project) else {
        panic!("expected a lockfile");
    };
    assert_eq!(
        found.ignored,
        [
            project.join("yarn.lock"),
            project.join("package-lock.json"),
            project.join("bun.lock")
        ]
    );
}
