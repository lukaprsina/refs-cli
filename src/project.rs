//! Project discovery and the sync-time checks on the project's output paths (spec §5, §6.1).

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::agent_file;
use crate::atomic;
use crate::config::{self, Config};
use crate::diagnostic::ProjectError;
use crate::exclude;
use crate::plan::{AgentFileText, ProjectObserved};
use crate::worktree::Worktree;

pub const CONFIG_FILE: &str = "refs.toml";

/// The nearest directory at or above `start` that holds a `refs.toml`, canonicalised.
pub fn find_root(start: &Path) -> Result<PathBuf, ProjectError> {
    let start = dunce::canonicalize(start).map_err(|source| ProjectError::Read {
        path: start.display().to_string(),
        source,
    })?;
    start
        .ancestors()
        .find(|dir| dir.join(CONFIG_FILE).is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| ProjectError::NoConfig {
            start: start.display().to_string(),
        })
}

/// Where `refs init` sets up a Project (spec §5): `start` itself with `here`; otherwise
/// the nearest directory up with a `refs.toml` when that is `start`, and an error when it is
/// further up (a nested project needs `--here`); with none, the git worktree root, or
/// `start` outside git.
pub fn init_root(start: &Path, here: bool) -> Result<PathBuf, ProjectError> {
    let canonical = dunce::canonicalize(start).map_err(|source| ProjectError::Read {
        path: start.display().to_string(),
        source,
    })?;
    if here {
        return Ok(canonical);
    }
    match find_root(&canonical) {
        Ok(root) if root == canonical => Ok(root),
        Ok(root) => Err(ProjectError::Nested {
            start: canonical.display().to_string(),
            root: root.display().to_string(),
        }),
        Err(ProjectError::NoConfig { .. }) => {
            Ok(Worktree::locate(&canonical).map_or(canonical, |tree| tree.top))
        }
        Err(e) => Err(e),
    }
}

/// The text of `<root>/refs.toml`.
pub fn read_config(root: &Path) -> Result<String, ProjectError> {
    std::fs::read_to_string(root.join(CONFIG_FILE)).map_err(|source| ProjectError::Read {
        path: CONFIG_FILE.into(),
        source,
    })
}

/// Replace `<root>/refs.toml` with `text`, atomically.
pub fn write_config(root: &Path, text: &str) -> Result<(), ProjectError> {
    atomic::write(&root.join(CONFIG_FILE), text).map_err(|source| ProjectError::Write {
        path: CONFIG_FILE.into(),
        source,
    })
}

/// Find the project from `start`, read and validate its config, and check its output
/// paths, all before anything is written. Every problem found is returned.
pub fn load(start: &Path) -> Result<(PathBuf, Config), Vec<miette::Report>> {
    let root = find_root(start).map_err(|e| vec![e.into()])?;
    let text = read_config(&root).map_err(|e| vec![e.into()])?;
    let config = config::parse(&text).map_err(|e| vec![e.into()])?;
    let errors = check_outputs(&root, &config);
    if errors.is_empty() {
        Ok((root, config))
    } else {
        Err(errors.into_iter().map(Into::into).collect())
    }
}

/// The §6.1 checks on the output paths, run before anything is written: each must stay
/// inside the project once symlinks are resolved, with no broken symlink on the way; an
/// existing `references_dir` must be a directory and each Agent file a regular file. Not
/// race-resistant, as the spec says. Reports every problem.
pub fn check_outputs(root: &Path, config: &Config) -> Vec<ProjectError> {
    let mut errors = Vec::new();
    let Ok(canonical_root) = dunce::canonicalize(root) else {
        return errors;
    };
    let dir = config.settings.references_dir().to_string();
    if check_inside(root, &canonical_root, &dir, &mut errors)
        && root.join(&dir).exists()
        && !root.join(&dir).is_dir()
    {
        errors.push(ProjectError::NotADirectory { path: dir });
    }
    for file in config.settings.agents_files() {
        if check_inside(root, &canonical_root, &file, &mut errors)
            && root.join(&file).exists()
            && !root.join(&file).is_file()
        {
            errors.push(ProjectError::NotAFile { path: file });
        }
    }
    errors
}

/// Walk `relative` from the root one component at a time. A symlink on the way must resolve
/// to somewhere inside the root; the first component that does not exist ends the walk, so
/// the nearest existing ancestor is what gets checked. Returns whether the path is fine.
fn check_inside(
    root: &Path,
    canonical_root: &Path,
    relative: &str,
    errors: &mut Vec<ProjectError>,
) -> bool {
    let mut current = root.to_path_buf();
    for component in Path::new(relative).components() {
        current.push(component);
        let meta = match std::fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(e) if e.kind() == ErrorKind::NotFound => return true,
            Err(source) => {
                errors.push(ProjectError::Read {
                    path: relative.into(),
                    source,
                });
                return false;
            }
        };
        if meta.file_type().is_symlink() {
            match dunce::canonicalize(&current) {
                Ok(target) if target.starts_with(canonical_root) => {}
                Ok(_) => {
                    errors.push(ProjectError::EscapesRoot {
                        path: relative.into(),
                    });
                    return false;
                }
                Err(_) => {
                    errors.push(ProjectError::BrokenSymlink {
                        path: relative.into(),
                    });
                    return false;
                }
            }
        }
    }
    true
}

/// What the Project's own files say before stage 2 plans against them: the Agent files'
/// text and whether the exclude rule is in place.
pub fn observe(root: &Path, config: &Config) -> Result<ProjectObserved, miette::Report> {
    let references_dir = config.settings.references_dir().to_string();
    let mut agent_files = Vec::new();
    for path in config.settings.agents_files() {
        let text = agent_file::read(&root.join(&path))?;
        agent_files.push(AgentFileText { path, text });
    }
    let exclude = exclude::observe(root, &references_dir)?;
    Ok(ProjectObserved {
        references_dir,
        agent_files,
        exclude,
    })
}

/// Add the exclude rule for `references_dir` to the Project's git exclude file, unless it is
/// already there.
pub fn ensure_exclude(root: &Path, references_dir: &str) -> Result<(), ProjectError> {
    exclude::ensure(root, references_dir)
}
