//! The Outline of a repo at a Ref, and what `add` and `update` ask of it (ADR 0011): the
//! directories for `paths`, the docs files for `start`, which files a Checkout will hold at all,
//! and what a new Ref breaks. Pure over the listed paths, so none of it needs git; only the
//! Manifests are read through the `Source`.

use std::collections::BTreeSet;
use std::fmt;

use crate::config::{Repo, RepoRef};
use crate::diagnostic::SourceError;
use crate::packages::{self, infer, manifest_paths};
use crate::source::{Pin, Source};
use crate::ui::Say;

/// The most directories or files a picker lists, shallowest first.
const MAX_LISTED: usize = 5000;

/// What a remote holds at a ref: the Pin it resolved to and every path of the commit's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    pub pin: Pin,
    pub paths: Vec<String>,
}

impl Outline {
    /// The directories of the tree, in path order, and how many were left out past the cap. Dot
    /// directories and what is below them are not offered; `--paths` can still name one.
    pub fn directories(&self) -> (Vec<String>, usize) {
        let all: BTreeSet<String> = (self.paths.iter())
            .flat_map(|path| path.match_indices('/').map(|(i, _)| path[..i].to_owned()))
            .filter(|dir| !dir.split('/').any(dotted))
            .collect();
        let (mut dirs, left) = shallowest(all.into_iter().collect(), MAX_LISTED);
        dirs.sort();
        (dirs, left)
    }

    /// The docs files a Checkout of `chosen` will hold, in path order, and how many were left
    /// out past the cap: Markdown, MDX, reStructuredText and AsciiDoc files, and any README.
    pub fn start_files(&self, chosen: &[String]) -> (Vec<String>, usize) {
        let docs = |path: &&String| {
            let file = path.rsplit('/').next().unwrap_or(path);
            let ext = file.rsplit_once('.').map(|(_, ext)| ext.to_lowercase());
            file.starts_with("README")
                || ext
                    .is_some_and(|e| ["md", "mdx", "markdown", "rst", "adoc"].contains(&e.as_str()))
        };
        let found = self.paths.iter().filter(docs).filter(|p| held(p, chosen));
        let (mut files, left) = shallowest(found.cloned().collect(), MAX_LISTED);
        files.sort();
        (files, left)
    }

    /// What the tree lacks of `repo`'s `paths` and `start` (the Break rule).
    pub fn gone(&self, repo: &Repo) -> Gone {
        gone(&self.paths, repo)
    }

    /// The Manifests of the tree that a Checkout of `paths` will hold, up to the cap (said when
    /// some were left unread). The sync may resolve another commit if upstream moved in
    /// between: accepted.
    pub fn manifests(
        &self,
        source: &dyn Source,
        paths: &[String],
        say: &mut dyn Say,
    ) -> Result<Manifests, SourceError> {
        let (chosen, left) = manifest_paths(&self.paths, paths);
        if left > 0 {
            let read = chosen.len();
            say.status(format!("read {read} Manifests and left {left} unread"));
        }
        let read = source.read(self.pin.url(), &self.pin, &chosen)?;
        Ok(Manifests(read))
    }
}

/// The Manifests (path and text) of an Outline that were read, once, for what `add` and
/// `update` ask about `packages` and the description; `packages` reads the text. Empty when
/// none was, which is also what a repo with none gives.
#[derive(Debug, Default)]
pub struct Manifests(Vec<(String, String)>);

impl Manifests {
    /// The names of the packages that a Checkout of `paths` gives.
    pub fn packages_for(&self, paths: &[String]) -> Vec<String> {
        packages::packages_for(&self.0, paths)
    }

    /// The description of the root Manifest, if it has one.
    pub fn description(&self) -> Option<String> {
        infer(&self.0).description
    }
}

/// Whether `path` is below the directory `dir`.
pub(crate) fn below(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// Whether `path` is `dir` or below it.
pub(crate) fn within(path: &str, dir: &str) -> bool {
    path == dir || below(path, dir)
}

/// Whether `dir` is a dot directory (`.github`): tooling, not what the Repo documents.
fn dotted(dir: &str) -> bool {
    dir.starts_with('.')
}

/// Directories that hold dependencies, build output, examples or test fixtures, and dot
/// directories: what is in them is not what the Repo documents.
fn skipped(dir: &str) -> bool {
    const SKIPPED: [&str; 8] = [
        "node_modules",
        "target",
        "vendor",
        "dist",
        "examples",
        "fixtures",
        "test",
        "tests",
    ];
    dotted(dir) || SKIPPED.contains(&dir)
}

/// Whether the file at `path` is one a Checkout of `chosen` (the Repo's `paths`; empty is the
/// whole repo) will hold and the Repo documents. Under cone mode a Checkout holds the files at
/// the root, the files along the way to a chosen path, and everything under it. A skipped
/// directory hides what is in it, unless the person chose the directory or one above it.
pub fn held(path: &str, chosen: &[String]) -> bool {
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let clean = |dir: &str| !dir.split('/').any(skipped);
    let in_checkout = chosen.is_empty()
        || dir.is_empty()
        || chosen.iter().any(|c| within(dir, c) || within(c, dir));
    in_checkout
        && (clean(dir)
            || chosen
                .iter()
                .any(|c| within(dir, c) && clean(&dir[c.len()..])))
}

/// At most `cap` of `found`, the shallowest first, and how many more there were.
pub fn shallowest(mut found: Vec<String>, cap: usize) -> (Vec<String>, usize) {
    found.sort_by_key(|path| (path.matches('/').count(), path.clone()));
    let left = found.len().saturating_sub(cap);
    found.truncate(cap);
    (found, left)
}

/// `options` with each of `ticked` among them, in path order: a ticked one the cap cut off, or
/// that the tree lacks, is still offered, so that leaving it ticked changes nothing.
pub fn with_ticked(mut options: Vec<String>, ticked: &[String]) -> Vec<String> {
    let missing: Vec<String> = ticked
        .iter()
        .filter(|t| !options.contains(t))
        .cloned()
        .collect();
    options.extend(missing);
    options.sort();
    options
}

/// `dirs` without those below another of them: cone mode includes the child anyway.
pub fn drop_children(dirs: Vec<String>) -> Vec<String> {
    let all = dirs.clone();
    dirs.into_iter()
        .filter(|dir| !all.iter().any(|other| other != dir && within(dir, other)))
        .collect()
}

/// What a tree lacks of a Repo's chosen `paths` and `start`: a `path` that is no directory in
/// it, a `start` that is no file in it. Read from the Outline of a new Ref it is advisory, as
/// `verify` still decides at the commit `sync` resolves.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Gone {
    pub paths: Vec<String>,
    pub start: Vec<String>,
}

impl Gone {
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.start.is_empty()
    }
}

/// What `files` lacks of `repo`'s `paths` and `start`.
fn gone(files: &[String], repo: &Repo) -> Gone {
    let missing = |chosen: &[String], found: &dyn Fn(&String) -> bool| {
        chosen.iter().filter(|c| !found(c)).cloned().collect()
    };
    Gone {
        paths: missing(&repo.path_strings(), &|dir| {
            files.iter().any(|file| below(file, dir))
        }),
        start: missing(&repo.start_strings(), &|file| files.contains(file)),
    }
}

/// The Break rule as the check `sync` makes at the `pin` it resolved: one error for each
/// of the Repo's `paths` and `start` that `files` (`Source::files`) lacks, telling one of the
/// wrong kind from a missing one. A submodule or a symlink is listed as a file.
pub fn verify(repo: RepoRef, pin: &Pin, files: &[String]) -> Vec<SourceError> {
    let gone = gone(files, repo.repo);
    let (id, sha) = (repo.id.to_string(), pin.sha().to_string());
    let paths = gone.paths.into_iter().map(|path| {
        let (repo, sha) = (id.clone(), sha.clone());
        match files.contains(&path) {
            true => SourceError::PathNotDir { repo, path, sha },
            false => SourceError::PathMissing { repo, path, sha },
        }
    });
    let start = gone.start.into_iter().map(|path| {
        let (repo, sha) = (id.clone(), sha.clone());
        match files.iter().any(|file| below(file, &path)) {
            true => SourceError::StartNotFile { repo, path, sha },
            false => SourceError::StartMissing { repo, path, sha },
        }
    });
    paths.chain(start).collect()
}

/// `paths `a`, `b`; start `c``
impl fmt::Display for Gone {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let list = |items: &[String]| {
            let quoted: Vec<_> = items.iter().map(|i| format!("`{i}`")).collect();
            quoted.join(", ")
        };
        let parts = [("paths", &self.paths), ("start", &self.start)]
            .into_iter()
            .filter(|(_, items)| !items.is_empty())
            .map(|(label, items)| format!("{label} {}", list(items)));
        write!(f, "{}", parts.collect::<Vec<_>>().join("; "))
    }
}
