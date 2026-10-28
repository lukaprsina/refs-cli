//! Package lockfiles (`Cargo.lock`, `package-lock.json`, ...): which versions of a package a
//! project resolved. refs only reads them (glossary: Package lockfile, Used version).

use std::path::Path;

use super::{Ecosystem, Package};
use crate::worktree::candidate_dirs;

mod bun;
mod cargo;
mod npm_spec;
mod package_lock;
mod pnpm;
mod poetry;
mod python;
mod uv;
mod yarn;

/// The dependency groups of a `package.json`, which the npm formats that record the project's own
/// dependencies (`package-lock.json`, `bun.lock`) read to tell which versions it asks for.
const DEPENDENCY_GROUPS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

/// Reads one Package lockfile format: the versions of a package in the file's text.
type Parser = fn(name: &str, text: &str) -> Result<Vec<Used>, String>;

/// Every Package lockfile refs reads: its ecosystem, file name and format. Within an ecosystem
/// the order is the one in which they are preferred when a directory holds more than one.
const LOCKFILES: [(Ecosystem, &str, Parser); 7] = [
    (Ecosystem::Npm, "pnpm-lock.yaml", pnpm::used),
    (Ecosystem::Npm, "yarn.lock", yarn::used),
    (Ecosystem::Npm, "package-lock.json", package_lock::used),
    (Ecosystem::Npm, "bun.lock", bun::used),
    (Ecosystem::Cargo, "Cargo.lock", cargo::used),
    (Ecosystem::Pypi, "uv.lock", uv::used),
    (Ecosystem::Pypi, "poetry.lock", poetry::used),
];

/// A version of a package that a Package lockfile resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Used {
    pub version: String,
    /// Whether the format says the project itself asks for the package, not only something it
    /// depends on. `false` also when the format cannot say.
    pub direct: bool,
}

/// Every distinct registry version of `name` in `text`, the document of the Package lockfile
/// called `file` (a file name, not a path), lowest first. A version the project asks for itself
/// is `direct` even if another entry of that version is not. Packages from git, a path or a
/// workspace are not registry versions and are left out. Errors when `file` is not a Package
/// lockfile refs reads, or `text` is not its document.
pub fn used_versions(file: &str, name: &str, text: &str) -> Result<Vec<Used>, String> {
    let (_, _, parser) = LOCKFILES
        .iter()
        .find(|(_, known, _)| *known == file)
        .ok_or(format!("refs does not read {file}"))?;
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
    /// One version to use: the only one, else the only one the format marks direct.
    One {
        file: String,
        version: String,
        /// The Package lockfiles that were not read.
        ignored: Vec<String>,
    },
    /// Several, lowest first, and nothing to choose between them. Never a guess.
    Several {
        file: String,
        versions: Vec<String>,
        ignored: Vec<String>,
    },
}

/// What the Package lockfile of the project at `root` says about `package`. The nearest
/// directory (see `candidate_dirs`) with a Package lockfile of the ecosystem is the one that is
/// read, whether or not it has the package; in it, the first of the ecosystem's Package lockfiles
/// that exists.
pub fn answer(package: &Package, root: &Path) -> Answer {
    let name = |file: &Path| {
        let file = file.strip_prefix(root).unwrap_or(file);
        file.display().to_string()
    };
    for dir in candidate_dirs(root) {
        let mut present = LOCKFILES
            .iter()
            .filter(|(ecosystem, ..)| *ecosystem == package.ecosystem)
            .map(|&(_, lockfile, _)| (lockfile, dir.join(lockfile)))
            .filter(|(_, path)| path.is_file());
        let Some((lockfile, path)) = present.next() else {
            continue;
        };
        let file = name(&path);
        let ignored = present.map(|(_, path)| name(&path)).collect();
        let used = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| used_versions(lockfile, &package.name, &text));
        let used = match used {
            Ok(used) => used,
            Err(why) => return Answer::Unreadable { file, why },
        };
        // The one version the project asks for, if exactly one is; otherwise all of them.
        let direct: Vec<&Used> = used.iter().filter(|used| used.direct).collect();
        let choices = if direct.len() == 1 {
            direct
        } else {
            used.iter().collect()
        };
        return match choices[..] {
            [] => Answer::Absent { file, ignored },
            [only] => Answer::One {
                file,
                version: only.version.clone(),
                ignored,
            },
            _ => Answer::Several {
                file,
                versions: choices.iter().map(|used| used.version.clone()).collect(),
                ignored,
            },
        };
    }
    Answer::Missing
}
