//! `Source` backed by the system `git` binary. Every git command goes through `command::Cmd`, which
//! is where the safety rules of spec §7.7 and the version floor of §7.5 are enforced, so no
//! call site can skip them.

pub mod cache;
mod command;
pub mod remote;

use std::path::{Path, PathBuf};

use crate::config::{RepoRef, is_full_sha};
use crate::diagnostic::SourceError;
use crate::source::{MaterialiseOpts, Observed, Pin, PinKind, Source, VerifyOpts};
use cache::{Cache, Repo};
use command::Cmd;
use remote::EntryKind;

pub struct GitSource {
    cache: Cache,
    /// `<project>/<references_dir>`: where Checkouts are made.
    checkouts: PathBuf,
}

impl GitSource {
    pub fn new(cache_root: PathBuf, checkouts: PathBuf) -> GitSource {
        GitSource {
            cache: Cache::new(&cache_root),
            checkouts,
        }
    }
}

impl Source for GitSource {
    fn resolve(&self, repo: RepoRef) -> Result<Pin, SourceError> {
        let url = repo.repo.url.as_ref().as_str();
        let git_ref = repo.repo.effective_ref();
        remote::check_input(url, git_ref)?;
        if is_full_sha(git_ref) {
            return Ok(Pin::git(url, git_ref, git_ref, None));
        }
        if git_ref == "HEAD" {
            let out = Cmd::new()
                .args(["ls-remote", "--symref", "--"])
                .arg(url)
                .arg("HEAD")
                .run()?;
            let (sha, branch) = remote::select_head(url, &out)?;
            return Ok(Pin::git(url, git_ref, &sha, branch.as_deref()));
        }
        let patterns = [
            format!("refs/heads/{git_ref}"),
            format!("refs/tags/{git_ref}"),
            format!("refs/tags/{git_ref}^{{}}"),
        ];
        let out = Cmd::new()
            .args(["ls-remote", "--"])
            .arg(url)
            .args(&patterns)
            .run()?;
        let sha = remote::select_ref(url, git_ref, &out)?;
        Ok(Pin::git(url, git_ref, &sha, None))
    }

    fn verify(&self, repo: RepoRef, pin: &Pin, opts: VerifyOpts) -> Result<(), SourceError> {
        let PinKind::Git { url, sha, .. } = &pin.0;
        remote::check_sha(sha)?;
        self.cache.with_repo(url, sha, opts.offline, |cache| {
            cache.ensure_commit(url, sha, opts.offline)?;
            for path in &repo.repo.paths {
                let path = path.get_ref();
                match cache.entry_kind(sha, path)? {
                    EntryKind::Tree => {}
                    EntryKind::Missing => {
                        return Err(SourceError::PathMissing {
                            repo: repo.id.into(),
                            path: path.into(),
                            sha: sha.clone(),
                        });
                    }
                    EntryKind::Other => {
                        return Err(SourceError::PathNotDir {
                            repo: repo.id.into(),
                            path: path.into(),
                            sha: sha.clone(),
                        });
                    }
                }
            }
            for path in &repo.repo.start {
                let path = path.get_ref();
                if cache.entry_kind(sha, path)? == EntryKind::Missing {
                    return Err(SourceError::StartMissing {
                        repo: repo.id.into(),
                        path: path.into(),
                        sha: sha.clone(),
                    });
                }
            }
            Ok(())
        })
    }

    fn materialise(
        &self,
        repo: RepoRef,
        pin: &Pin,
        opts: MaterialiseOpts,
    ) -> Result<(), SourceError> {
        let PinKind::Git { url, sha, .. } = &pin.0;
        remote::check_sha(sha)?;
        let dest = self.checkouts.join(repo.id);
        let paths = repo.repo.path_strings();
        self.cache.with_repo(url, sha, opts.offline, |cache| {
            // Checked under the lock, so what is discarded on failure is only what this call made.
            if std::fs::symlink_metadata(&dest).is_ok() {
                return Err(SourceError::Failed {
                    message: format!(
                        "{} already exists; moving a Checkout lands in #8",
                        dest.display()
                    ),
                });
            }
            cache.ensure_commit(url, sha, opts.offline)?;
            if !opts.offline {
                cache.prefetch(sha, &paths)?;
            }
            add_worktree(cache, repo.id, &dest, sha, &paths).inspect_err(|_| discard(cache, &dest))
        })
    }

    fn remove(&self, _: &str) -> Result<(), SourceError> {
        not_implemented()
    }

    fn inspect(&self, _: &str) -> Result<Observed, SourceError> {
        not_implemented()
    }

    fn list(&self) -> Result<Vec<String>, SourceError> {
        not_implemented()
    }
}

/// Checkouts, inspection and removal land in #8.
fn not_implemented<T>() -> Result<T, SourceError> {
    Err(SourceError::Failed {
        message: "git support is not implemented yet".into(),
    })
}

/// Create the Checkout: a detached, sparse worktree of the Cache repository at `dest`, never
/// fetching lazily. A missing blob (offline, or a prefetch that fell short) is reported
/// naming the object.
fn add_worktree(
    cache: &Repo,
    id: &str,
    dest: &Path,
    sha: &str,
    paths: &[String],
) -> Result<(), SourceError> {
    cache.git().args(["worktree", "prune"]).run()?;
    cache
        .git()
        .args(["worktree", "add", "--no-checkout", "--detach", "--"])
        .arg(dest)
        .arg(sha)
        .run()?;
    if !paths.is_empty() {
        Cmd::new()
            .dir(dest)
            .args(["sparse-checkout", "set", "--cone", "--"])
            .args(paths)
            .run()?;
    }
    Cmd::new()
        .dir(dest)
        .no_lazy_fetch()
        .args(["checkout", "--detach"])
        .arg(sha)
        .run()
        .map(drop)
        .map_err(|failure| match remote::missing_object(&failure.stderr) {
            Some(oid) => SourceError::ObjectMissing {
                repo: id.into(),
                oid,
            },
            None => failure.into(),
        })
}

/// Undo a Checkout that failed half way: nothing of it may stay, on disk or in the Cache's
/// worktree registrations. Best effort: the error being reported is the original one.
fn discard(cache: &Repo, dest: &Path) {
    let _ = std::fs::remove_dir_all(dest);
    let _ = cache.git().args(["worktree", "prune"]).run();
}
