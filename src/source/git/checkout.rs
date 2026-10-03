//! A Checkout on disk: how to tell one refs made from a stranger's directory, and the record
//! kept of what it holds. Everything here reads; `GitSource` changes things.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::remote::{lexical_path, parse_gitdir, worktree_entry};
use crate::diagnostic::SourceError;
use crate::source::Pin;

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
    /// The Cache entry (its directory name under the Cache root).
    pub entry: String,
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
    let entry = worktree_entry(&admin, cache_root).or_else(|| {
        let canonical = cache_root.canonicalize().ok()?;
        worktree_entry(&admin, &canonical)
    });
    Ok(match entry {
        None => Layout::Foreign,
        Some(_) if !admin.is_dir() => Layout::Dangling,
        Some(entry) => Layout::Linked(Link { entry, admin }),
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
