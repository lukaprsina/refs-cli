//! Package lockfiles (`Cargo.lock`, `package-lock.json`, ...): which versions of a package a
//! project resolved. refs only reads them (glossary: Package lockfile, Used version).

use std::path::{Path, PathBuf};

use super::Ecosystem;
use crate::worktree::candidate_dirs;

mod cargo;
mod package_lock;
mod pnpm;
mod yaml;
mod yarn;

/// The lockfile formats refs can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    CargoLock,
    PackageLock,
    Pnpm,
    Yarn,
}

/// A version of a package that a lockfile resolved.
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
        Format::Pnpm => pnpm::used(name, text),
        Format::Yarn => yarn::used(name, text),
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

impl Format {
    /// The Package lockfiles of `ecosystem`, as file name and format, in the order they are
    /// preferred when a directory holds more than one.
    fn of(ecosystem: Ecosystem) -> &'static [(&'static str, Format)] {
        match ecosystem {
            Ecosystem::Cargo => &[("Cargo.lock", Format::CargoLock)],
            Ecosystem::Npm => &[
                ("pnpm-lock.yaml", Format::Pnpm),
                ("yarn.lock", Format::Yarn),
                ("package-lock.json", Format::PackageLock),
            ],
            // `uv.lock` and `poetry.lock` are not read yet.
            Ecosystem::Pypi => &[],
        }
    }
}

/// The Package lockfile a project uses and the versions of one package in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub file: PathBuf,
    pub format: Format,
    /// Empty when the lockfile does not have the package.
    pub used: Vec<Used>,
    /// Lockfiles of the same ecosystem in the same directory that were not read, in order of
    /// preference.
    pub ignored: Vec<PathBuf>,
}

/// What looking for a package in a project's Package lockfile came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Found(Found),
    /// No lockfile of the ecosystem from the project directory up to the git worktree top.
    Missing,
    /// The nearest lockfile could not be read.
    Unreadable {
        file: PathBuf,
        why: String,
    },
}

/// Look for `name` in the Package lockfile of the project at `project_dir`. The nearest
/// directory (see `candidate_dirs`) with a lockfile of the ecosystem is the one that is read,
/// whether or not it has the package; in it, the first of the ecosystem's lockfiles that
/// exists.
pub fn find(ecosystem: Ecosystem, name: &str, project_dir: &Path) -> Lookup {
    let formats = Format::of(ecosystem);
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
            .and_then(|text| used_versions(format, name, &text));
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
