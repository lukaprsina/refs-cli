//! The git lifecycle of spec §12, through the `refs` binary against local `file://` remotes:
//! one Cache, real worktrees, real exit codes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A remote with `docs/a.md` and `src/b.rs` on `main`.
struct Remote {
    dir: TempDir,
}

impl Remote {
    fn new() -> Remote {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["config", "uploadpack.allowFilter", "true"]);
        let remote = Remote { dir };
        remote.commit("docs/a.md", "a");
        remote.commit("src/b.rs", "b");
        remote
    }

    fn commit(&self, file: &str, text: &str) {
        let path = self.dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        git(self.dir.path(), &["add", "."]);
        git(self.dir.path(), &["commit", "-q", "-m", file]);
    }

    fn url(&self) -> String {
        format!("file://{}", self.dir.path().display())
    }
}

/// A project that is a git repository, with `refs.toml` set by `configure`.
struct Project {
    dir: TempDir,
    cache: PathBuf,
}

struct Run {
    code: Option<i32>,
    stderr: String,
}

impl Project {
    fn new(cache: &Path) -> Project {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q"]);
        Project {
            dir,
            cache: cache.to_path_buf(),
        }
    }

    fn configure(&self, repos: &[(&str, &Remote, &str, &[&str])]) {
        let text: String = repos
            .iter()
            .map(|(id, remote, git_ref, paths)| {
                let paths: Vec<_> = paths.iter().map(|p| format!("\"{p}\"")).collect();
                format!(
                    "[repos.{id}]\nurl = \"{}\"\nref = \"{git_ref}\"\npaths = [{}]\n\n",
                    remote.url(),
                    paths.join(", ")
                )
            })
            .collect();
        fs::write(self.dir.path().join("refs.toml"), text).unwrap();
    }

    fn command(&self, root: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_refs"));
        command
            .args(["--no-color"])
            .args(args)
            .current_dir(root)
            .env("XDG_CACHE_HOME", &self.cache);
        command
    }

    fn run(&self, args: &[&str]) -> Run {
        Self::collect(self.command(self.dir.path(), args).output().unwrap())
    }

    fn collect(out: Output) -> Run {
        Run {
            code: out.status.code(),
            stderr: String::from_utf8(out.stderr).unwrap(),
        }
    }

    fn ok(&self, args: &[&str]) -> Run {
        let run = self.run(args);
        assert_eq!(run.code, Some(0), "refs {args:?}: {}", run.stderr);
        run
    }

    fn checkout(&self, id: &str) -> PathBuf {
        self.dir.path().join(".references").join(id)
    }

    fn read(&self, id: &str, file: &str) -> Option<String> {
        fs::read_to_string(self.checkout(id).join(file)).ok()
    }
}

#[test]
fn lock_sync_change_ref_upgrade_and_remove() {
    let remote = Remote::new();
    git(remote.dir.path(), &["branch", "other"]);
    let cache = TempDir::new().unwrap();
    let project = Project::new(cache.path());

    project.configure(&[("r", &remote, "main", &["docs"])]);
    project.ok(&["lock"]);
    project.ok(&["sync"]);
    assert_eq!(project.read("r", "docs/a.md").as_deref(), Some("a"));
    assert_eq!(project.read("r", "src/b.rs"), None, "only the sparse paths");
    project.ok(&["sync", "--check"]);

    // A new commit on the floating ref changes nothing until the lock is upgraded.
    remote.commit("docs/new.md", "new");
    project.ok(&["sync"]);
    assert_eq!(project.read("r", "docs/new.md"), None);
    project.ok(&["lock", "--upgrade"]);
    assert_eq!(project.run(&["sync", "--check"]).code, Some(3));
    project.ok(&["sync"]);
    assert_eq!(project.read("r", "docs/new.md").as_deref(), Some("new"));

    // A different ref moves the checkout back to that commit.
    project.configure(&[("r", &remote, "other", &["docs"])]);
    project.ok(&["lock"]);
    project.ok(&["sync"]);
    assert_eq!(project.read("r", "docs/new.md"), None);
    assert_eq!(project.read("r", "docs/a.md").as_deref(), Some("a"));

    // Removed from the config, the checkout goes on the next sync.
    project.configure(&[]);
    project.ok(&["sync"]);
    assert!(!project.checkout("r").exists());
    project.ok(&["sync", "--check"]);
}

#[test]
fn a_moved_project_directory_is_repaired_by_the_next_sync() {
    let remote = Remote::new();
    let cache = TempDir::new().unwrap();
    let project = Project::new(cache.path());
    project.configure(&[("r", &remote, "main", &["docs"])]);
    project.ok(&["sync"]);

    let parent = TempDir::new().unwrap();
    let moved = parent.path().join("moved");
    fs::rename(project.dir.path(), &moved).unwrap();
    let run = Project::collect(project.command(&moved, &["sync"]).output().unwrap());

    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        !run.stderr.contains("refs::sync::recreated"),
        "{}",
        run.stderr
    );
    assert_eq!(
        fs::read_to_string(moved.join(".references/r/docs/a.md")).unwrap(),
        "a"
    );
    let run = Project::collect(
        project
            .command(&moved, &["sync", "--check"])
            .output()
            .unwrap(),
    );
    assert_eq!(run.code, Some(0), "{}", run.stderr);
}

#[test]
fn two_projects_share_one_cache_with_different_sparse_paths() {
    let remote = Remote::new();
    let cache = TempDir::new().unwrap();
    let (first, second) = (Project::new(cache.path()), Project::new(cache.path()));
    first.configure(&[("r", &remote, "main", &["docs"])]);
    second.configure(&[("r", &remote, "main", &["src"])]);

    first.ok(&["sync"]);
    second.ok(&["sync"]);

    assert_eq!(first.read("r", "docs/a.md").as_deref(), Some("a"));
    assert_eq!(first.read("r", "src/b.rs"), None);
    assert_eq!(second.read("r", "src/b.rs").as_deref(), Some("b"));
    assert_eq!(second.read("r", "docs/a.md"), None);
    first.ok(&["sync", "--check"]);
    second.ok(&["sync", "--check"]);

    // One project going away leaves the other's checkout alone.
    first.configure(&[]);
    first.ok(&["sync"]);
    assert!(!first.checkout("r").exists());
    second.ok(&["sync", "--check"]);
    assert_eq!(second.read("r", "src/b.rs").as_deref(), Some("b"));
}

#[test]
fn concurrent_syncs_of_one_project_both_succeed() {
    let remote = Remote::new();
    let cache = TempDir::new().unwrap();
    let project = Project::new(cache.path());
    project.configure(&[("r", &remote, "main", &["docs"])]);

    let runs: Vec<Run> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..3)
            .map(|_| {
                scope.spawn(|| {
                    Project::collect(
                        project
                            .command(project.dir.path(), &["sync"])
                            .output()
                            .unwrap(),
                    )
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    for run in &runs {
        assert_eq!(run.code, Some(0), "{}", run.stderr);
    }
    assert_eq!(project.read("r", "docs/a.md").as_deref(), Some("a"));
    project.ok(&["sync", "--check"]);
}
