//! Where a directory sits in a git worktree, from one `git rev-parse`.

use std::path::{Path, PathBuf};

use crate::source::git::command::Cmd;

/// A directory's place in a git worktree.
#[derive(Debug, PartialEq, Eq)]
pub struct Worktree {
    /// The top of the worktree.
    pub top: PathBuf,
    /// The git dir shared by all worktrees of the repository.
    pub common_dir: PathBuf,
    /// The directory's path below `top`, with a trailing `/`; empty at the top.
    pub prefix: String,
}

impl Worktree {
    /// Ask git where `dir` is. `None` when it is not in a worktree (a bare repository, the
    /// inside of a `.git`), or when git's answer cannot be read back safely.
    pub fn locate(dir: &Path) -> Option<Worktree> {
        let out = Cmd::new()
            .dir(dir)
            .args(["rev-parse", "--path-format=absolute"])
            .args(["--is-inside-work-tree", "--show-toplevel"])
            .args(["--git-common-dir", "--show-prefix"])
            .run()
            .ok()?;
        // A path with a newline in it would split into more lines, and so is not trusted.
        let answer: Vec<&str> = out.strip_suffix('\n')?.split('\n').collect();
        let [inside, top, common, prefix] = answer[..] else {
            return None;
        };
        (inside == "true").then(|| Worktree {
            top: PathBuf::from(top),
            common_dir: PathBuf::from(common),
            prefix: prefix.to_string(),
        })
    }
}

/// The directories a tool's config or Package lockfile for the project at `project_dir` may sit in,
/// nearest first: `project_dir`, then each parent up to the top of its git worktree. Outside a
/// worktree it is `project_dir` alone.
pub fn candidate_dirs(project_dir: &Path) -> Vec<PathBuf> {
    let top = Worktree::locate(project_dir).and_then(|tree| dunce::canonicalize(tree.top).ok());
    let mut dirs = Vec::new();
    for dir in project_dir.ancestors() {
        dirs.push(dir.to_path_buf());
        if top.as_deref().is_none_or(|top| dir == top) {
            return dirs;
        }
    }
    // The top is not above `project_dir` (a path git and the file system spell differently).
    vec![project_dir.to_path_buf()]
}
