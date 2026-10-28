//! Package lockfiles (`Cargo.lock`, `package-lock.json`, ...): which versions of a package a
//! project resolved. refs only reads them (glossary: Package lockfile, Used version).

use std::path::Path;

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

/// A version of a package that a Package lockfile resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Used {
    pub version: String,
    /// Whether the format says the project itself asks for the package, not only something it
    /// depends on. `false` also when the format cannot say.
    pub direct: bool,
}

/// Every distinct registry version of `name` in `text`, the document of the Package lockfile
/// called `file` (a file name, not a path), lowest first. A version the project asks for itself is `direct` even if
/// another entry of that version is not. Packages from git, a path or a workspace are not
/// registry versions and are left out. Errors when `file` is not a Package lockfile refs reads,
/// or `text` is not its document.
pub fn used_versions(file: &str, name: &str, text: &str) -> Result<Vec<Used>, String> {
    let parser = match file {
        "Cargo.lock" => cargo::used,
        "package-lock.json" => package_lock::used,
        "bun.lock" => bun::used,
        "pnpm-lock.yaml" => pnpm::used,
        "yarn.lock" => yarn::used,
        "uv.lock" => uv::used,
        "poetry.lock" => poetry::used,
        _ => return Err(format!("refs does not read {file}")),
    };
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
    let lockfiles = package.ecosystem.spec().lockfiles;
    let name = |file: &Path| {
        let file = file.strip_prefix(root).unwrap_or(file);
        file.display().to_string()
    };
    for dir in candidate_dirs(root) {
        let mut present = lockfiles
            .iter()
            .map(|&file| (file, dir.join(file)))
            .filter(|(_, path)| path.is_file());
        let Some((kind, path)) = present.next() else {
            continue;
        };
        let (file, ignored) = (name(&path), present.map(|(_, path)| name(&path)).collect());
        let read = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| used_versions(kind, &package.name, &text));
        let used = match read {
            Ok(used) => used,
            Err(why) => return Answer::Unreadable { file, why },
        };
        let mut direct = used.iter().filter(|used| used.direct);
        return match (&used[..], direct.next(), direct.next()) {
            ([], ..) => Answer::Absent { file, ignored },
            ([only], ..) | (_, Some(only), None) => Answer::One {
                file,
                version: only.version.clone(),
                ignored,
            },
            (all, ..) => Answer::Several {
                file,
                versions: all.iter().map(|used| used.version.clone()).collect(),
                ignored,
            },
        };
    }
    Answer::Missing
}
