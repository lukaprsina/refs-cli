//! `Source` backed by the system `git` binary. Every git command goes through `command::Cmd`, which
//! is where the safety rules of spec §7.7 and the version floor of §7.5 are enforced, so no
//! call site can skip them.

pub mod cache;
pub mod checkout;
pub(crate) mod command;
pub mod remote;

use std::path::{Path, PathBuf};

use crate::config::{RepoRef, is_full_sha};
use crate::diagnostic::SourceError;
use crate::source::{MaterialiseOpts, Observed, Pin, Source, VerifyOpts};
use cache::{Cache, Repo};
use checkout::{Layout, Record};
use command::Cmd;
use remote::{EntryKind, cache_dir_name, dirty_files};

pub struct GitSource {
    cache: Cache,
    /// `<project>/<references_dir>`: where Checkouts are made.
    checkouts: PathBuf,
}

impl GitSource {
    /// The Source for a project: the Cache in the user's cache directory (found from the
    /// environment) and the Checkouts in `checkouts`.
    pub fn from_env(checkouts: PathBuf) -> Result<GitSource, SourceError> {
        let local_app_data = if cfg!(windows) {
            std::env::var("LOCALAPPDATA").ok()
        } else {
            None
        };
        let cache = cache::cache_root(
            std::env::var("XDG_CACHE_HOME").ok().as_deref(),
            local_app_data.as_deref(),
            std::env::var("HOME").ok().as_deref(),
        )
        .ok_or_else(|| SourceError::Failed {
            message: "cannot find the cache directory: set XDG_CACHE_HOME (or LOCALAPPDATA on \n                      Windows, or HOME)"
                .into(),
        })?;
        Ok(GitSource::new(cache, checkouts))
    }

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
        let (url, sha) = (pin.url(), pin.sha());
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
                            sha: sha.to_string(),
                        });
                    }
                    EntryKind::Other => {
                        return Err(SourceError::PathNotDir {
                            repo: repo.id.into(),
                            path: path.into(),
                            sha: sha.to_string(),
                        });
                    }
                }
            }
            for path in &repo.repo.start {
                let path = path.get_ref();
                let (id, path, sha) = (repo.id.to_string(), path.to_string(), sha.to_string());
                match cache.entry_kind(&sha, &path)? {
                    EntryKind::Missing => {
                        return Err(SourceError::StartMissing {
                            repo: id,
                            path,
                            sha,
                        });
                    }
                    // cone mode checks out the root's files, not its directories
                    EntryKind::Tree if !repo.repo.paths.is_empty() && !path.contains('/') => {
                        return Err(SourceError::StartNotFile {
                            repo: id,
                            path,
                            sha,
                        });
                    }
                    _ => {}
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
        let (url, sha) = (pin.url(), pin.sha());
        remote::check_sha(sha)?;
        let dest = self.checkouts.join(repo.id);
        let cache_name = cache_dir_name(url);
        let paths = repo.repo.path_strings();
        // Everything that can fail on the network comes first, so that a failure leaves an
        // existing Checkout alone.
        self.cache.with_repo(url, sha, opts.offline, |cache| {
            cache.ensure_commit(url, sha, opts.offline)?;
            if opts.offline {
                return Ok(());
            }
            cache.prefetch(sha, &paths)
        })?;
        // A Checkout of another remote cannot move here: its objects are in another Cache.
        if let Layout::Linked(link) = checkout::layout(&dest, self.cache.root())?
            && link.cache_name != cache_name
        {
            self.remove(repo.id)?;
        }
        self.cache.with_repo(url, sha, opts.offline, |cache| {
            // Checked under the lock, so what is discarded on failure is only what this call made.
            let moving = match checkout::layout(&dest, self.cache.root())? {
                Layout::Absent => false,
                Layout::Linked(link) if link.cache_name == cache_name => true,
                _ => {
                    return Err(SourceError::Failed {
                        message: format!(
                            "{} is not a Checkout of this Repo; refs leaves it alone",
                            dest.display()
                        ),
                    });
                }
            };
            // A move is a fresh Checkout of the new commit: the blobs prefetched are exactly
            // those it reads, which changing the sparse paths and the commit in place would
            // not guarantee.
            if moving {
                remove_worktree(cache, &dest)?;
            }
            add_worktree(cache, repo.id, &dest, sha, &paths)
                .and_then(|()| record(&self.cache, &dest, pin, paths))
                .inspect_err(|_| discard(cache, &dest))
        })
    }

    fn remove(&self, id: &str) -> Result<(), SourceError> {
        let dest = self.checkouts.join(id);
        match checkout::layout(&dest, self.cache.root())? {
            Layout::Absent => Ok(()),
            Layout::Foreign => Err(SourceError::Failed {
                message: format!("{} was not made by refs; leaving it alone", dest.display()),
            }),
            // Its history is gone with the Cache, and `.git` says refs made it.
            Layout::Dangling => std::fs::remove_dir_all(&dest).map_err(|e| SourceError::Failed {
                message: format!("could not remove {}: {e}", dest.display()),
            }),
            Layout::Linked(link) => self
                .cache
                .with_named(&link.cache_name, |cache| remove_worktree(cache, &dest)),
        }
    }

    fn inspect(&self, id: &str) -> Result<Observed, SourceError> {
        let dest = self.checkouts.join(id);
        match checkout::layout(&dest, self.cache.root())? {
            Layout::Absent => Ok(Observed::Absent),
            Layout::Foreign => Ok(Observed::Foreign),
            Layout::Dangling => Ok(Observed::Dangling),
            Layout::Linked(link) => self
                .cache
                .with_named(&link.cache_name, |cache| observe(cache, &dest, &link.admin)),
        }
    }

    fn list(&self) -> Result<Vec<String>, SourceError> {
        let entries = match std::fs::read_dir(&self.checkouts) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => {
                return Err(SourceError::Failed {
                    message: format!("could not read {}: {e}", self.checkouts.display()),
                });
            }
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort();
        Ok(names)
    }
}

/// What is on disk at `dest`, a worktree of `cache`. First repairs the link a moved project
/// directory broke (silently, spec §7.3), so that the next prune does not forget it.
fn observe(cache: &Repo, dest: &Path, admin: &Path) -> Result<Observed, SourceError> {
    let recorded = std::fs::read_to_string(admin.join("gitdir")).unwrap_or_default();
    let here = dest.join(".git");
    let here = dunce::canonicalize(&here).unwrap_or(here);
    if Path::new(recorded.trim()) != here {
        // If it cannot be repaired the Checkout is read as it is; a pruned one is dangling.
        let _ = cache.git().args(["worktree", "repair"]).arg(dest).run();
    }
    let head = Cmd::new()
        .dir(dest)
        .no_lazy_fetch()
        .args(["rev-parse", "HEAD"])
        .run()?
        .trim()
        .to_string();
    // A record of another commit (someone moved HEAD by hand) is not what is checked out.
    let record = Record::read(admin).filter(|r| r.pin.sha() == head);
    let (pin, paths) = match record {
        Some(record) => (record.pin, record.paths),
        None => (Pin::git("", "", &head, None), vec![]),
    };
    // `--no-optional-locks`: looking must not write, not even the index refresh.
    let status = Cmd::new()
        .dir(dest)
        .no_lazy_fetch()
        .args(["--no-optional-locks", "status", "--porcelain=v1", "-z"])
        .arg("--no-renames")
        .run()?;
    Ok(Observed::At {
        pin,
        paths,
        dirty_files: dirty_files(&status),
    })
}

/// Note what the Checkout at `dest` was made from, beside git's own record of it.
fn record(cache: &Cache, dest: &Path, pin: &Pin, paths: Vec<String>) -> Result<(), SourceError> {
    match checkout::layout(dest, cache.root())? {
        Layout::Linked(link) => Record {
            pin: pin.clone(),
            paths,
        }
        .write(&link.admin),
        _ => Err(SourceError::Failed {
            message: format!("{} is not a worktree of the cache", dest.display()),
        }),
    }
}

/// Remove the worktree and forget its registration. `--force`: it holds generated copies, and
/// whether edits in it matter is for `plan` to have decided.
fn remove_worktree(cache: &Repo, dest: &Path) -> Result<(), SourceError> {
    cache
        .git()
        .args(["worktree", "remove", "--force", "--"])
        .arg(dest)
        .run()?;
    cache.git().args(["worktree", "prune"]).run()?;
    Ok(())
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
