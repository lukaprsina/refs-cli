//! The Cache (spec §7.1): one bare, blobless clone per normalised URL, each behind an
//! exclusive file lock. Everything that touches a Cache repository runs inside
//! `Cache::with_repo`, so two `refs` processes never mutate one at the same time.

use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::command::Cmd;
use super::remote::{
    EntryKind, ancestor_dirs, cache_dir_name, commit_unavailable, entry_kind, missing_oids,
    normalise_url, tree_blobs,
};
use crate::diagnostic::SourceError;

/// `$XDG_CACHE_HOME/refs`, or `~/.cache/refs`. A relative `XDG_CACHE_HOME` is ignored, as the
/// XDG specification says.
pub fn cache_root(xdg_cache_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let base = match xdg_cache_home.map(Path::new) {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        _ => Path::new(home.filter(|h| !h.is_empty())?).join(".cache"),
    };
    Some(base.join("refs"))
}

pub struct Cache {
    /// `<cache_root>/git`
    root: PathBuf,
}

fn failed(message: String) -> SourceError {
    SourceError::Failed { message }
}

fn io(what: &str, path: &Path, source: std::io::Error) -> SourceError {
    failed(format!("{what} {}: {source}", path.display()))
}

impl Cache {
    pub fn new(cache_root: &Path) -> Cache {
        Cache {
            root: cache_root.join("git"),
        }
    }

    /// `<cache_root>/git`: where the Cache repositories are, and where a worktree's admin
    /// directory must be to be one of ours.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run `f` while holding the lock of the Cache entry `name`, waiting for any other holder.
    fn locked<T>(
        &self,
        name: &str,
        f: impl FnOnce() -> Result<T, SourceError>,
    ) -> Result<T, SourceError> {
        fs::create_dir_all(&self.root).map_err(|e| io("could not create", &self.root, e))?;
        let lock_path = self.root.join(format!("{name}.lock"));
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| io("could not open", &lock_path, e))?;
        let mut lock = fd_lock::RwLock::new(file);
        let _held = lock
            .write()
            .map_err(|e| io("could not lock", &lock_path, e))?;
        f()
    }

    /// Run `f` on the Cache repository for `url` while holding its lock. A missing repository
    /// is cloned, unless `offline`: then it is an error naming `sha`, the commit the caller
    /// wanted.
    pub fn with_repo<T>(
        &self,
        url: &str,
        sha: &str,
        offline: bool,
        f: impl FnOnce(&Repo) -> Result<T, SourceError>,
    ) -> Result<T, SourceError> {
        let name = cache_dir_name(url);
        self.locked(&name, || {
            let dir = self.root.join(&name);
            let url_file = self.root.join(format!("{name}.url"));
            if dir.exists() {
                Self::check_url(&url_file, url)?;
            } else if offline {
                return Err(SourceError::NotCached {
                    url: url.into(),
                    sha: sha.into(),
                });
            } else {
                self.clone_repo(url, &name)?;
            }
            if !url_file.exists() {
                crate::atomic::write(&url_file, &format!("{url}\n"))
                    .map_err(|e| io("could not write", &url_file, e))?;
            }
            f(&Repo { dir })
        })
    }

    /// Run `f` on the existing Cache repository `name` while holding its lock. For what a
    /// Checkout needs without knowing a URL: the entry is the one its `.git` names.
    pub fn with_entry<T>(
        &self,
        name: &str,
        f: impl FnOnce(&Repo) -> Result<T, SourceError>,
    ) -> Result<T, SourceError> {
        self.locked(name, || {
            let dir = self.root.join(name);
            if !dir.is_dir() {
                return Err(failed(format!("the cache entry {} is gone", dir.display())));
            }
            f(&Repo { dir })
        })
    }

    /// The file beside a Cache repository names the URL it was cloned from. Two URLs with
    /// one hash would share a repository and mix their objects, so a different one is an
    /// error.
    fn check_url(url_file: &Path, url: &str) -> Result<(), SourceError> {
        match fs::read_to_string(url_file) {
            Ok(stored) if normalise_url(stored.trim()) != normalise_url(url) => {
                Err(failed(format!(
                    "the cache entry for {url} belongs to {}; remove {} to clear it",
                    stored.trim(),
                    url_file.display()
                )))
            }
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io("could not read", url_file, e)),
        }
    }

    /// Clone into a temporary directory and rename it, so a clone that died half way is never
    /// mistaken for a Cache repository.
    fn clone_repo(&self, url: &str, name: &str) -> Result<(), SourceError> {
        let tmp = self.root.join(format!("{name}.tmp"));
        let dir = self.root.join(name);
        if tmp.exists() {
            fs::remove_dir_all(&tmp).map_err(|e| io("could not remove", &tmp, e))?;
        }
        Cmd::new()
            .args(["clone", "--bare", "--filter=blob:none", "--"])
            .arg(url)
            .arg(&tmp)
            .run()?;
        fs::rename(&tmp, &dir).map_err(|e| io("could not move", &tmp, e))
    }
}

/// One Cache repository, reached only through `Cache::with_repo`.
pub struct Repo {
    dir: PathBuf,
}

impl Repo {
    /// A command on this repository.
    pub fn git(&self) -> Cmd {
        Cmd::new().git_dir(&self.dir)
    }

    /// The type of `oid` if the repository has it. Never fetches.
    fn object_type(&self, oid: &str) -> Option<String> {
        self.git()
            .no_lazy_fetch()
            .args(["cat-file", "-t", oid])
            .run()
            .ok()
            .map(|out| out.trim().to_string())
    }

    /// Make sure `sha` is a commit in the repository, fetching it (with its trees, no
    /// blobs) unless `offline`. A tag object is refused: `worktree add` and `ls-tree` would
    /// peel it silently, so the Lock would name a different commit than the one checked out.
    pub fn ensure_commit(&self, url: &str, sha: &str, offline: bool) -> Result<(), SourceError> {
        let mut kind = self.object_type(sha);
        if kind.is_none() {
            if offline {
                return Err(SourceError::NotCached {
                    url: url.into(),
                    sha: sha.into(),
                });
            }
            self.fetch_commit(url, sha)?;
            kind = self.object_type(sha);
        }
        match kind.as_deref() {
            Some("commit") => Ok(()),
            Some("tag") => Err(SourceError::UnpeeledTag { sha: sha.into() }),
            Some(other) => Err(failed(format!("{sha} is a {other}, not a commit"))),
            None => Err(SourceError::CommitUnavailable {
                url: url.into(),
                sha: sha.into(),
            }),
        }
    }

    fn fetch_commit(&self, url: &str, sha: &str) -> Result<(), SourceError> {
        self.git()
            .args(["fetch", "--no-tags", "--filter=blob:none", "origin", sha])
            .run()
            .map(drop)
            .map_err(|failure| {
                if commit_unavailable(&failure.stderr) {
                    SourceError::CommitUnavailable {
                        url: url.into(),
                        sha: sha.into(),
                    }
                } else {
                    failure.into()
                }
            })
    }

    /// What is at `path` in the tree of `sha`.
    pub fn entry_kind(&self, sha: &str, path: &str) -> Result<EntryKind, SourceError> {
        let out = self
            .git()
            .no_lazy_fetch()
            .args(["ls-tree", sha, "--", path])
            .run()?;
        Ok(entry_kind(&out))
    }

    /// Fetch every blob a checkout of `sha` with `paths` reads, in one round trip, so that
    /// `checkout` never has to fetch lazily (spec §7.3 step 1b). Cone mode checks out the
    /// files directly in the root and in every ancestor directory of a path, as well as
    /// everything under the paths.
    pub fn prefetch(&self, sha: &str, paths: &[String]) -> Result<(), SourceError> {
        let tree = |args: &[&str], paths: &[String]| -> Result<Vec<String>, SourceError> {
            let mut cmd = self
                .git()
                .no_lazy_fetch()
                .arg("ls-tree")
                .args(args)
                .arg(sha);
            if !paths.is_empty() {
                cmd = cmd.arg("--").args(paths);
            }
            Ok(tree_blobs(&cmd.run()?))
        };
        let mut wanted = tree(&[], &[])?;
        if paths.is_empty() {
            wanted.extend(tree(&["-r"], &[])?);
        } else {
            let ancestors = ancestor_dirs(paths);
            if !ancestors.is_empty() {
                wanted.extend(tree(&[], &ancestors)?);
            }
            wanted.extend(tree(&["-r"], paths)?);
        }
        let present = self
            .git()
            .no_lazy_fetch()
            .args(["cat-file", "--batch-check"])
            .stdin(wanted.join("\n") + "\n")
            .run()?;
        let missing = missing_oids(&present);
        if missing.is_empty() {
            return Ok(());
        }
        // The options git's own lazy fetch uses. Without the `noop` negotiation, GitHub
        // answers a want for a blob with "did not send all necessary objects".
        self.git()
            .args(["-c", "fetch.negotiationAlgorithm=noop", "fetch"])
            .args([
                "--no-tags",
                "--no-write-fetch-head",
                "--recurse-submodules=no",
            ])
            .args(["--filter=blob:none", "origin", "--stdin"])
            .stdin(missing.join("\n") + "\n")
            .run()?;
        Ok(())
    }
}
