//! The Cache (spec §7.1): one bare repository per normalised URL, filled with shallow, blobless
//! fetches of the pinned commits (ADR 0010), each behind an exclusive file lock. Everything
//! that touches a Cache repository runs inside `Cache::with_repo`, so two `refs` processes
//! never mutate one at the same time.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::command::Cmd;
use super::remote::{
    EntryKind, ancestor_dirs, cache_dir_name, commit_unavailable, entry_kind, missing_oids,
    normalise_url, tree_blobs,
};
use crate::diagnostic::SourceError;

/// `$XDG_CACHE_HOME/refs`, else `%LOCALAPPDATA%\refs` where the caller passes one (Windows),
/// else `~/.cache/refs`. A relative `XDG_CACHE_HOME` is ignored, as the XDG specification says.
pub fn cache_root(
    xdg_cache_home: Option<&str>,
    local_app_data: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    fn absolute(dir: Option<&str>) -> Option<&Path> {
        dir.map(Path::new).filter(|dir| dir.is_absolute())
    }
    let base = match absolute(xdg_cache_home).or_else(|| absolute(local_app_data)) {
        Some(dir) => dir.to_path_buf(),
        None => Path::new(home.filter(|h| !h.is_empty())?).join(".cache"),
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

/// `path` as git spells it (symlinks and Windows short names resolved), though some of it may
/// not exist: the deepest existing ancestor is resolved and the rest appended. A worktree's
/// `.git` names its admin directory that way, and must still be placed once the Cache is gone.
fn resolved(path: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut head = path;
    loop {
        if let Ok(real) = dunce::canonicalize(head) {
            return tail.iter().rev().fold(real, |p, name| p.join(name));
        }
        match (head.parent(), head.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name);
                head = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

impl Cache {
    pub fn new(cache_root: &Path) -> Cache {
        Cache {
            root: resolved(cache_root).join("git"),
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
    /// is created empty, unless `offline`: then it is an error naming `sha`, the commit the
    /// caller wanted.
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
                self.init_repo(url, &name)?;
            }
            if !url_file.exists() {
                crate::atomic::write(&url_file, &format!("{url}\n"))
                    .map_err(|e| io("could not write", &url_file, e))?;
            }
            f(&Repo { dir })
        })
    }

    /// Run `f` on the existing Cache repository `name` while holding its lock. For what a
    /// Checkout needs without knowing a URL: it is the one its `.git` names.
    pub fn with_named<T>(
        &self,
        name: &str,
        f: impl FnOnce(&Repo) -> Result<T, SourceError>,
    ) -> Result<T, SourceError> {
        self.locked(name, || {
            let dir = self.root.join(name);
            if !dir.is_dir() {
                return Err(failed(format!(
                    "the cache repository {} is gone",
                    dir.display()
                )));
            }
            f(&Repo { dir })
        })
    }

    /// The file beside a Cache repository names the URL the entry was made for. Two URLs with
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

    /// Create an empty bare repository that knows `url` (and has no fetch refspec) in a
    /// temporary directory and rename it, so one that died half way is never mistaken for a
    /// Cache repository. Commits arrive later, one Pin at a time (`Repo::ensure_commit`).
    fn init_repo(&self, url: &str, name: &str) -> Result<(), SourceError> {
        let tmp = self.root.join(format!("{name}.tmp"));
        let dir = self.root.join(name);
        if tmp.exists() {
            fs::remove_dir_all(&tmp).map_err(|e| io("could not remove", &tmp, e))?;
        }
        Cmd::new()
            .args(["init", "--bare", "--quiet", "--"])
            .arg(&tmp)
            .run()?;
        Cmd::new()
            .git_dir(&tmp)
            .args(["config", "remote.origin.url", url])
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
            .args([
                "fetch",
                "--no-tags",
                "--depth=1",
                "--filter=blob:none",
                "origin",
                sha,
            ])
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
    /// everything under the paths. With `offline` nothing is fetched: a blob that is not here
    /// is an error naming it and `repo`, before anything is replaced.
    pub fn prefetch(
        &self,
        repo: &str,
        sha: &str,
        paths: &[String],
        offline: bool,
    ) -> Result<(), SourceError> {
        let tree = |args: &[&str], paths: &[String]| -> Result<Vec<String>, SourceError> {
            let mut cmd = self
                .git()
                .no_lazy_fetch()
                .args(["ls-tree", "-z"])
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
        let missing = self.missing(&wanted)?;
        match missing.first() {
            Some(oid) if offline => Err(SourceError::ObjectMissing {
                repo: repo.into(),
                oid: oid.clone(),
            }),
            _ => self.fetch(&missing),
        }
    }

    /// The ids of `wanted` that the repository does not have.
    fn missing(&self, wanted: &[String]) -> Result<Vec<String>, SourceError> {
        let present = self
            .git()
            .no_lazy_fetch()
            .args(["cat-file", "--batch-check"])
            .stdin(wanted.join("\n") + "\n")
            .run()?;
        Ok(missing_oids(&present))
    }

    /// Fetch the blobs `missing` (object ids), in one round trip.
    fn fetch(&self, missing: &[String]) -> Result<(), SourceError> {
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
            .run()
            .map(drop)
            .map_err(|failure| {
                if commit_unavailable(&failure.stderr) {
                    SourceError::ObjectsUnavailable
                } else {
                    failure.into()
                }
            })
    }

    /// Every path in the tree of `sha`, in git's order.
    pub fn paths(&self, sha: &str) -> Result<Vec<String>, SourceError> {
        let out = self
            .git()
            .no_lazy_fetch()
            .args(["ls-tree", "-r", "--name-only", "-z", sha])
            .run()?;
        Ok(out
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(String::from)
            .collect())
    }

    /// The text of the files at `paths` in the tree of `sha`, in the order asked. Their blobs
    /// are fetched in one batch, then read without fetching lazily. A path that is not a
    /// file of the tree is an error.
    pub fn read(&self, sha: &str, paths: &[String]) -> Result<Vec<(String, String)>, SourceError> {
        if paths.is_empty() {
            return Ok(vec![]);
        }
        let listing = self
            .git()
            .no_lazy_fetch()
            .args(["ls-tree", "-z", sha, "--"])
            .args(paths)
            .run()?;
        let entries: HashMap<&str, &str> = listing
            .split('\0')
            .filter_map(|line| line.split_once('\t'))
            .map(|(meta, path)| (path, meta))
            .collect();
        let mut oids = Vec::with_capacity(paths.len());
        for path in paths {
            let mut meta = entries.get(path.as_str()).copied().unwrap_or("").split(' ');
            match (meta.nth(1), meta.next()) {
                (Some("blob"), Some(oid)) => oids.push(oid.to_string()),
                _ => return Err(failed(format!("{path} is not a file in {sha}"))),
            }
        }
        self.fetch(&self.missing(&oids)?)?;
        let out = self
            .git()
            .no_lazy_fetch()
            .args(["cat-file", "--batch"])
            .stdin(oids.join("\n") + "\n")
            .run_bytes()?;
        // Each blob is `<oid> blob <size>\n<size bytes>\n`.
        let mut rest = out.as_slice();
        let mut texts = Vec::with_capacity(paths.len());
        for path in paths {
            let framed = (|| {
                let end = rest.iter().position(|&b| b == b'\n')?;
                let size: usize = std::str::from_utf8(&rest[..end])
                    .ok()?
                    .rsplit(' ')
                    .next()?
                    .parse()
                    .ok()?;
                Some((rest.get(end + 1..end + 1 + size)?, end + 2 + size))
            })();
            let Some((body, next)) = framed else {
                return Err(failed(format!("could not read {path} in {sha}")));
            };
            texts.push((path.clone(), String::from_utf8_lossy(body).into_owned()));
            rest = rest.get(next..).unwrap_or_default();
        }
        Ok(texts)
    }
}
