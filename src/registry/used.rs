//! Package lockfiles (`Cargo.lock`, `package-lock.json`, ...): which versions of a package a
//! project resolved. refs only reads them (glossary: Package lockfile, Used version).

use std::path::{Path, PathBuf};

use super::{Ecosystem, Package};
use crate::worktree::candidate_dirs;

pub(crate) mod bun;
pub(crate) mod cargo;
mod npm_spec;
pub(crate) mod package_lock;
pub(crate) mod pnpm;
pub(crate) mod poetry;
mod python;
pub(crate) mod uv;
pub(crate) mod yarn;

/// The dependency groups of a `package.json`, which the npm formats that record the project's own
/// dependencies (`package-lock.json`, `bun.lock`) read to tell which versions it asks for.
const DEPENDENCY_GROUPS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

/// Reads one Package lockfile format: the versions of a package in the file's text.
pub type Parser = fn(name: &str, text: &str) -> Result<Vec<Used>, String>;

/// A version of a package that a Package lockfile resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Used {
    pub version: String,
    /// Whether the format says the project itself asks for the package, not only something it
    /// depends on. `false` also when the format cannot say.
    pub direct: bool,
}

/// Every distinct registry version of `name` in `text`, the document of the Package lockfile
/// called `file`, lowest first. A version the project asks for itself is `direct` even if
/// another entry of that version is not. Packages from git, a path or a workspace are not
/// registry versions and are left out. Errors when `file` is not a Package lockfile refs reads,
/// or `text` is not its document.
pub fn used_versions(file: &str, name: &str, text: &str) -> Result<Vec<Used>, String> {
    let parser = Ecosystem::ALL
        .iter()
        .flat_map(|ecosystem| ecosystem.spec().lockfiles)
        .find(|(known, _)| *known == file)
        .map(|&(_, parser)| parser);
    parse(
        parser.ok_or(format!("refs does not read {file}"))?,
        name,
        text,
    )
}

fn parse(parser: Parser, name: &str, text: &str) -> Result<Vec<Used>, String> {
    Ok(distinct(parser(name, text)?))
}

/// `found` with each version once (direct if any entry of it is), lowest first. A version that
/// is not semver sorts after those that are, by its text.
fn distinct(found: Vec<Used>) -> Vec<Used> {
    let mut distinct: Vec<Used> = Vec::new();
    for used in found {
        match distinct.iter_mut().find(|d| d.version == used.version) {
            Some(same) => same.direct |= used.direct,
            None => distinct.push(used),
        }
    }
    distinct.sort_by_cached_key(|used| {
        let parsed = semver::Version::parse(&used.version).ok();
        (parsed.is_none(), parsed, used.version.clone())
    });
    distinct
}

/// The Package lockfile a project uses and the versions of one package in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub file: PathBuf,
    /// Empty when the Package lockfile does not have the package.
    pub used: Vec<Used>,
    /// Package lockfiles of the same ecosystem in the same directory that were not read, in order of
    /// preference.
    pub ignored: Vec<PathBuf>,
}

/// Which version of a package to use, out of what a Package lockfile has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick<'a> {
    /// The Package lockfile does not have the package.
    Nothing,
    /// One version, the only one or the only one the project asks for itself.
    One(&'a str),
    /// Several, lowest first, and nothing to choose between them.
    Several(Vec<&'a str>),
}

impl Found {
    /// The version to use: the only one, else the only one the format marks direct. Never a
    /// guess between several.
    pub fn pick(&self) -> Pick<'_> {
        let mut direct = self.used.iter().filter(|used| used.direct);
        match (&self.used[..], direct.next(), direct.next()) {
            ([], ..) => Pick::Nothing,
            ([only], ..) | (_, Some(only), None) => Pick::One(&only.version),
            (all, ..) => Pick::Several(all.iter().map(|used| used.version.as_str()).collect()),
        }
    }
}

/// What looking for a package in a project's Package lockfile came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Found(Found),
    /// No Package lockfile of the ecosystem from the project directory up to the git worktree top.
    Missing,
    /// The nearest Package lockfile could not be read.
    Unreadable {
        file: PathBuf,
        why: String,
    },
}

/// What the project's Package lockfile says about a package, with the files named as a message
/// names them: relative to the project if they are below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// No Package lockfile of the ecosystem.
    Missing,
    /// The nearest Package lockfile could not be read.
    Unreadable { file: String, why: String },
    /// The Package lockfile does not have the package.
    Absent { file: String, ignored: Vec<String> },
    /// One version to use (see `Found::pick`).
    One {
        file: String,
        version: String,
        /// The Package lockfiles that were not read.
        ignored: Vec<String>,
    },
    /// Several, lowest first, and nothing to choose between them.
    Several {
        file: String,
        versions: Vec<String>,
        ignored: Vec<String>,
    },
}

/// What the Package lockfile of the project at `root` says about `package`: `find`, then
/// `Found::pick`.
pub fn answer(package: &Package, root: &Path) -> Answer {
    let name = |file: &Path| {
        let file = file.strip_prefix(root).unwrap_or(file);
        file.display().to_string()
    };
    let found = match find(package, root) {
        Lookup::Found(found) => found,
        Lookup::Missing => return Answer::Missing,
        Lookup::Unreadable { file, why } => {
            let file = name(&file);
            return Answer::Unreadable { file, why };
        }
    };
    let file = name(&found.file);
    let ignored = found.ignored.iter().map(|file| name(file)).collect();
    match found.pick() {
        Pick::Nothing => Answer::Absent { file, ignored },
        Pick::One(version) => Answer::One {
            file,
            version: version.to_owned(),
            ignored,
        },
        Pick::Several(versions) => Answer::Several {
            file,
            versions: versions.into_iter().map(str::to_owned).collect(),
            ignored,
        },
    }
}

/// Look for `package` in the Package lockfile of the project at `project_dir`. The nearest
/// directory (see `candidate_dirs`) with a Package lockfile of the ecosystem is the one that is read,
/// whether or not it has the package; in it, the first of the ecosystem's Package lockfiles that
/// exists.
pub fn find(package: &Package, project_dir: &Path) -> Lookup {
    let lockfiles = package.ecosystem.spec().lockfiles;
    for dir in candidate_dirs(project_dir) {
        let mut present = lockfiles
            .iter()
            .map(|&(file, parser)| (dir.join(file), parser))
            .filter(|(path, _)| path.is_file());
        let Some((file, parser)) = present.next() else {
            continue;
        };
        let ignored = present.map(|(path, _)| path).collect();
        let read = std::fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|text| parse(parser, &package.name, &text));
        return match read {
            Ok(used) => Lookup::Found(Found {
                file,
                used,
                ignored,
            }),
            Err(why) => Lookup::Unreadable { file, why },
        };
    }
    Lookup::Missing
}
