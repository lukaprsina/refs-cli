//! `GitSource::resolve` against real git, on local repositories.

use std::path::{Path, PathBuf};
use std::process::Command;

use miette::Diagnostic;
use refs_cli::config::{Repo, RepoRef, parse};
use refs_cli::source::Pin;
use refs_cli::source::Source;
use refs_cli::source::git::GitSource;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A remote with two commits on `main`:
/// - `first`: tagged `light` (lightweight), `v1` (annotated) and `dup` (tag);
/// - `second`: `main`, `dev` and a branch also called `dup`.
struct Remote {
    dir: TempDir,
    first: String,
    second: String,
}

impl Remote {
    fn new() -> Remote {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["commit", "-q", "--allow-empty", "-m", "first"]);
        let first = git(p, &["rev-parse", "HEAD"]);
        git(p, &["tag", "light"]);
        git(p, &["tag", "-a", "-m", "release", "v1"]);
        git(p, &["tag", "dup"]);
        git(p, &["commit", "-q", "--allow-empty", "-m", "second"]);
        let second = git(p, &["rev-parse", "HEAD"]);
        git(p, &["branch", "dev"]);
        git(p, &["branch", "dup"]);
        Remote { dir, first, second }
    }

    fn url(&self) -> String {
        format!("file://{}", self.dir.path().display())
    }

    fn repo(&self, git_ref: Option<&str>) -> Repo {
        let git_ref = git_ref
            .map(|r| format!("ref = \"{r}\""))
            .unwrap_or_default();
        let text = format!("[repos.r]\nurl = \"{}\"\n{git_ref}\n", self.url());
        parse(&text).unwrap().repos.into_values().next().unwrap()
    }

    fn resolve(&self, git_ref: Option<&str>) -> Result<Pin, refs_cli::diagnostic::SourceError> {
        let repo = self.repo(git_ref);
        GitSource::new(PathBuf::new(), PathBuf::new()).resolve(RepoRef {
            id: "r",
            repo: &repo,
        })
    }

    fn pin(&self, git_ref: &str, sha: &str, branch: Option<&str>) -> Pin {
        Pin::git(&self.url(), git_ref, sha, branch)
    }
}

fn code(e: impl Diagnostic) -> String {
    e.code().expect("a code").to_string()
}

#[test]
fn a_branch_resolves_to_its_tip() {
    let r = Remote::new();
    assert_eq!(
        r.resolve(Some("dev")).unwrap(),
        r.pin("dev", &r.second, None)
    );
}

#[test]
fn a_lightweight_tag_resolves_to_its_commit() {
    let r = Remote::new();
    assert_eq!(
        r.resolve(Some("light")).unwrap(),
        r.pin("light", &r.first, None)
    );
}

#[test]
fn an_annotated_tag_resolves_to_a_commit_not_the_tag_object() {
    let r = Remote::new();
    let tag_object = git(r.dir.path(), &["rev-parse", "v1"]);
    assert_ne!(tag_object, r.first, "the fixture must have a tag object");
    assert_eq!(r.resolve(Some("v1")).unwrap(), r.pin("v1", &r.first, None));
}

#[test]
fn a_tag_wins_over_a_branch_of_the_same_name() {
    let r = Remote::new();
    assert_eq!(
        r.resolve(Some("dup")).unwrap(),
        r.pin("dup", &r.first, None)
    );
}

#[test]
fn head_records_the_remote_default_branch() {
    let r = Remote::new();
    assert_eq!(
        r.resolve(None).unwrap(),
        r.pin("HEAD", &r.second, Some("main"))
    );
}

#[test]
fn a_full_sha_is_taken_as_is() {
    let r = Remote::new();
    let sha = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(r.resolve(Some(sha)).unwrap(), r.pin(sha, sha, None));
}

#[test]
fn a_missing_ref_is_an_error() {
    let r = Remote::new();
    assert_eq!(
        code(r.resolve(Some("nope")).unwrap_err()),
        "refs::git::ref_not_found"
    );
}

#[test]
fn an_unreachable_remote_is_a_git_failure() {
    let r = Remote::new();
    let gone = Remote {
        dir: TempDir::new().unwrap(),
        first: r.first.clone(),
        second: r.second.clone(),
    };
    assert_eq!(code(gone.resolve(None).unwrap_err()), "refs::git::failed");
}
