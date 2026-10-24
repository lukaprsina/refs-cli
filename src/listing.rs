//! What `add` offers from the paths of a repo (ADR 0011): the directories for `paths`, the docs
//! files for `start`, and which files a Checkout will hold at all. Pure over the listed paths, so
//! none of it needs git.

use std::collections::BTreeSet;
use std::fmt;

/// The most directories or files a picker lists, shallowest first.
const MAX_LISTED: usize = 5000;

/// Whether `path` is `dir` or below it.
fn within(path: &str, dir: &str) -> bool {
    path == dir || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
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

/// The directories of the repo, in path order, and how many were left out past the cap. Dot
/// directories and what is below them are not offered; `--paths` can still name one.
pub fn directories(paths: &[String]) -> (Vec<String>, usize) {
    let all: BTreeSet<String> = paths
        .iter()
        .flat_map(|path| path.match_indices('/').map(|(i, _)| path[..i].to_owned()))
        .filter(|dir| !dir.split('/').any(dotted))
        .collect();
    let (mut dirs, left) = shallowest(all.into_iter().collect(), MAX_LISTED);
    dirs.sort();
    (dirs, left)
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

/// The docs files a Checkout of `chosen` will hold, in path order, and how many were left out
/// past the cap: Markdown, MDX, reStructuredText and AsciiDoc files, and any README.
pub fn start_files(paths: &[String], chosen: &[String]) -> (Vec<String>, usize) {
    let docs = |path: &&String| {
        let file = path.rsplit('/').next().unwrap_or(path);
        let ext = file.rsplit_once('.').map(|(_, ext)| ext.to_lowercase());
        file.starts_with("README")
            || ext.is_some_and(|e| ["md", "mdx", "markdown", "rst", "adoc"].contains(&e.as_str()))
    };
    let found = paths.iter().filter(docs).filter(|p| held(p, chosen));
    let (mut files, left) = shallowest(found.cloned().collect(), MAX_LISTED);
    files.sort();
    (files, left)
}

/// `dirs` without those below another of them: cone mode includes the child anyway.
pub fn drop_children(dirs: Vec<String>) -> Vec<String> {
    let all = dirs.clone();
    dirs.into_iter()
        .filter(|dir| !all.iter().any(|other| other != dir && within(dir, other)))
        .collect()
}

/// What a new tree (its listed file `paths`) lacks of a Repo's chosen `paths` and `start`: a
/// `path` that is no directory in it, a `start` that is no file in it. Advisory, and not the
/// check a sync must pass (`Source::verify`, which also tells a file from a directory).
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

/// What `listed` (the files of a new tree) lacks of `paths` and `start`.
pub fn gone(listed: &[String], paths: &[String], start: &[String]) -> Gone {
    let missing = |chosen: &[String], found: &dyn Fn(&String) -> bool| {
        chosen.iter().filter(|c| !found(c)).cloned().collect()
    };
    Gone {
        paths: missing(paths, &|dir| {
            listed.iter().any(|file| within(file, dir) && file != dir)
        }),
        start: missing(start, &|file| listed.contains(file)),
    }
}
