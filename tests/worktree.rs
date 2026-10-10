use std::fs;
use std::path::PathBuf;

mod common;

use refs_cli::worktree::candidate_dirs;
use tempfile::TempDir;

fn canonical(path: &std::path::Path) -> PathBuf {
    dunce::canonicalize(path).unwrap()
}

#[test]
fn inside_a_worktree_the_directories_run_from_the_project_up_to_its_top() {
    let repo = TempDir::new().unwrap();
    common::git(repo.path(), &["init", "-q"]);
    let project = repo.path().join("apps/web");
    fs::create_dir_all(&project).unwrap();
    let top = canonical(repo.path());

    let dirs = candidate_dirs(&canonical(&project));

    assert_eq!(dirs, [top.join("apps/web"), top.join("apps"), top]);
}

#[test]
fn at_the_top_of_a_worktree_it_is_the_top_only() {
    let repo = TempDir::new().unwrap();
    common::git(repo.path(), &["init", "-q"]);
    let top = canonical(repo.path());

    assert_eq!(candidate_dirs(&top), [top]);
}

#[test]
fn outside_a_worktree_it_is_the_project_directory_only() {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("a/b");
    fs::create_dir_all(&project).unwrap();
    let project = canonical(&project);

    assert_eq!(candidate_dirs(&project), [project]);
}
