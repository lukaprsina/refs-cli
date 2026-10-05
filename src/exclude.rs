//! The Exclude rule: the line that keeps the references directory out of default search,
//! kept in the repository's local git exclude file. `observe` and `ensure` find and read or
//! write the file.

use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use crate::diagnostic::ProjectError;
use crate::plan::Exclude;
use crate::worktree::Worktree;

/// Whether `rule` is a line of the exclude file `text`, ignoring surrounding whitespace.
fn rule_present(text: &str, rule: &str) -> bool {
    text.lines().any(|l| l.trim() == rule)
}

/// `text` with `rule` appended as a line; a missing final newline is supplied first.
fn with_rule(text: &str, rule: &str) -> String {
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(rule);
    out.push('\n');
    out
}

/// The exclude file for the project at `root` and the rule that anchors the references
/// directory below the worktree top. `None` outside a git worktree.
fn locate(root: &Path, references_dir: &str) -> Option<(PathBuf, String)> {
    let tree = Worktree::locate(root)?;
    Some((
        tree.common_dir.join("info/exclude"),
        format!("/{}{references_dir}/", tree.prefix),
    ))
}

/// The text of the exclude file at `path`; empty when there is none yet.
fn read(path: &Path) -> std::io::Result<String> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(String::new()),
        other => other,
    }
}

/// Whether the Exclude rule for `references_dir` is in place.
pub fn observe(root: &Path, references_dir: &str) -> Result<Exclude, ProjectError> {
    let Some((path, rule)) = locate(root, references_dir) else {
        return Ok(Exclude::NoGit);
    };
    match read(&path) {
        Ok(text) if rule_present(&text, &rule) => Ok(Exclude::Present),
        Ok(_) => Ok(Exclude::Missing),
        Err(source) => Err(ProjectError::Read {
            path: path.display().to_string(),
            source,
        }),
    }
}

/// Add the Exclude rule for `references_dir`, unless it is already there. Does nothing
/// outside a git worktree.
pub fn ensure(root: &Path, references_dir: &str) -> Result<(), ProjectError> {
    let Some((path, rule)) = locate(root, references_dir) else {
        return Ok(());
    };
    let write = |source| ProjectError::Write {
        path: path.display().to_string(),
        source,
    };
    let text = read(&path).map_err(write)?;
    if rule_present(&text, &rule) {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().expect("a file has a parent")).map_err(write)?;
    crate::atomic::write(&path, &with_rule(&text, &rule)).map_err(write)
}
