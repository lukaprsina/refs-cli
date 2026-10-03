//! A Checkout on disk: how to tell one refs made from a stranger's directory, and the record
//! kept of what it holds. Only the record is written here; `GitSource` changes the rest.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diagnostic::SourceError;
use crate::source::Pin;

/// The path in a Checkout's `.git` file (`gitdir: <path>`).
pub fn parse_gitdir(dot_git: &str) -> Option<&str> {
    let path = dot_git.lines().next()?.strip_prefix("gitdir:")?.trim();
    (!path.is_empty()).then_some(path)
}

/// `path` with `.` and `..` resolved by name alone, so a gitdir that no longer exists can
/// still be placed.
pub fn lexical_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The Cache repository a worktree's admin directory belongs to, if it is where git puts one:
/// `<cache_git_root>/<hash>/worktrees/<name>`.
pub fn worktree_entry(admin: &Path, cache_git_root: &Path) -> Option<String> {
    let rest = lexical_path(admin)
        .strip_prefix(lexical_path(cache_git_root))
        .ok()?
        .to_path_buf();
    let mut parts = rest.components().map(|c| c.as_os_str().to_str());
    let (entry, worktrees, name) = (parts.next()??, parts.next()??, parts.next()??);
    let is_entry = entry.len() == 16 && entry.bytes().all(|b| b.is_ascii_hexdigit());
    (is_entry && worktrees == "worktrees" && !name.is_empty() && parts.next().is_none())
        .then(|| entry.to_string())
}

/// What kind of thing is at a Checkout's path (spec §7.3 step 4).
pub enum Layout {
    Absent,
    /// Not made by refs: never touched.
    Foreign,
    /// Made by refs, but the Cache it was a worktree of is gone.
    Dangling,
    Linked(Link),
}

/// A worktree of a Cache repository.
pub struct Link {
    /// The Cache repository's directory name under the Cache root.
    pub cache_name: String,
    /// `<cache repo>/worktrees/<name>`, where git keeps what is private to this worktree.
    pub admin: PathBuf,
}

/// Classify `dest` by its `.git` file: a Checkout of ours points at a worktree directory
/// inside `cache_root`. A directory without one, or with one pointing anywhere else, is
/// foreign even if it is a clone of the very same remote.
pub fn layout(dest: &Path, cache_root: &Path) -> Result<Layout, SourceError> {
    let io = |what: &str, path: &Path, e: std::io::Error| SourceError::Failed {
        message: format!("{what} {}: {e}", path.display()),
    };
    match fs::symlink_metadata(dest) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Ok(Layout::Foreign),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Layout::Absent),
        Err(e) => return Err(io("could not read", dest, e)),
    }
    let dot_git = dest.join(".git");
    match fs::symlink_metadata(&dot_git) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(Layout::Foreign),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Layout::Foreign),
        Err(e) => return Err(io("could not read", &dot_git, e)),
    }
    let text = match fs::read_to_string(&dot_git) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::InvalidData => return Ok(Layout::Foreign),
        Err(e) => return Err(io("could not read", &dot_git, e)),
    };
    let Some(path) = parse_gitdir(&text) else {
        return Ok(Layout::Foreign);
    };
    // A relative path is relative to the Checkout (git can be told to write them).
    let admin = lexical_path(&dest.join(path));
    let cache_name = worktree_entry(&admin, cache_root).or_else(|| {
        let canonical = cache_root.canonicalize().ok()?;
        worktree_entry(&admin, &canonical)
    });
    Ok(match cache_name {
        None => Layout::Foreign,
        Some(_) if !admin.is_dir() => Layout::Dangling,
        Some(cache_name) => Layout::Linked(Link { cache_name, admin }),
    })
}

/// What a Checkout was made from. `Observed` needs the Pin (its ref and URL are not in the
/// worktree) and the Paths as given, not as git normalises them. Kept in the worktree's admin
/// directory, so it goes when git removes or prunes the worktree.
#[derive(Debug, Serialize, Deserialize)]
pub struct Record {
    pub pin: Pin,
    pub paths: Vec<String>,
}

const RECORD_FILE: &str = "refs-checkout.toml";

impl Record {
    /// `None` if there is none or it cannot be read: the Checkout then reads as out of date.
    pub fn read(admin: &Path) -> Option<Record> {
        toml::from_str(&fs::read_to_string(admin.join(RECORD_FILE)).ok()?).ok()
    }

    pub fn write(&self, admin: &Path) -> Result<(), SourceError> {
        let path = admin.join(RECORD_FILE);
        let text = toml::to_string(self).map_err(|e| SourceError::Failed {
            message: format!("could not encode the record of {}: {e}", admin.display()),
        })?;
        crate::atomic::write(&path, &text).map_err(|e| SourceError::Failed {
            message: format!("could not write {}: {e}", path.display()),
        })
    }
}
