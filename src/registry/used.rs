//! Package lockfiles (`Cargo.lock`, `package-lock.json`, ...): which versions of a package a
//! project resolved. refs only reads them (glossary: Package lockfile, Used version).

use std::path::{Path, PathBuf};

use super::Package;
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

/// The Package lockfile formats refs can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    CargoLock,
    PackageLock,
    Pnpm,
    Yarn,
    UvLock,
    PoetryLock,
    BunLock,
}

/// A version of a package that a Package lockfile resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Used {
    pub version: String,
    /// Whether the format says the project itself asks for the package, not only something it
    /// depends on. `false` also when the format cannot say.
    pub direct: bool,
}

/// Every distinct registry version of `name` in `text`, lowest first. A version the project
/// asks for itself is `direct` even if another entry of that version is not. Packages from git,
/// a path or a workspace are not registry versions and are left out. Errors when `text` is not
/// the format's document.
pub fn used_versions(format: Format, name: &str, text: &str) -> Result<Vec<Used>, String> {
    let found = match format {
        Format::CargoLock => cargo::used(name, text)?,
        Format::PackageLock => package_lock::used(name, text)?,
        Format::Pnpm => pnpm::used(name, text)?,
        Format::Yarn => yarn::used(name, text),
        Format::UvLock => uv::used(name, text)?,
        Format::PoetryLock => poetry::used(name, text)?,
        Format::BunLock => bun::used(name, text)?,
    };
    Ok(distinct(found))
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
    pub format: Format,
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

/// Look for `package` in the Package lockfile of the project at `project_dir`. The nearest
/// directory (see `candidate_dirs`) with a Package lockfile of the ecosystem is the one that is read,
/// whether or not it has the package; in it, the first of the ecosystem's Package lockfiles that
/// exists.
pub fn find(package: &Package, project_dir: &Path) -> Lookup {
    let formats = package.ecosystem.spec().lockfiles;
    for dir in candidate_dirs(project_dir) {
        let mut present = formats
            .iter()
            .map(|&(file, format)| (dir.join(file), format))
            .filter(|(path, _)| path.is_file());
        let Some((file, format)) = present.next() else {
            continue;
        };
        let ignored = present.map(|(path, _)| path).collect();
        let read = std::fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|text| used_versions(format, &package.name, &text));
        return match read {
            Ok(used) => Lookup::Found(Found {
                file,
                format,
                used,
                ignored,
            }),
            Err(why) => Lookup::Unreadable { file, why },
        };
    }
    Lookup::Missing
}
