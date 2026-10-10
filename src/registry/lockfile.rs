//! Package lockfiles (`Cargo.lock`, `package-lock.json`, ...): which versions of a package a
//! project resolved. refs only reads them (glossary: Package lockfile, Used version).

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
