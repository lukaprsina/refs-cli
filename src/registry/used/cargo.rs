//! `Cargo.lock`: `[[package]]` tables. A package with a `source` of `registry+...` or
//! `sparse+...` is a registry version; one without a `source` belongs to the project, and its
//! `dependencies` are what the project asks for.

use serde::Deserialize;

use super::Used;

#[derive(Deserialize)]
struct Lock {
    #[serde(default)]
    package: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    source: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}

/// Crate names are the same with `-` and `_`.
fn same_crate(a: &str, b: &str) -> bool {
    let canon = |s: &str| s.replace('_', "-");
    canon(a) == canon(b)
}

pub(super) fn used(name: &str, text: &str) -> Result<Vec<Used>, String> {
    let lock: Lock = toml::from_str(text).map_err(|e| e.to_string())?;
    // A dependency is `name`, or `name version` when the lockfile has several versions of it.
    let asked_for = |version: &str| {
        lock.package
            .iter()
            .filter(|package| package.source.is_none())
            .flat_map(|package| &package.dependencies)
            .any(|entry| {
                let mut parts = entry.split(' ');
                let crate_name = parts.next().unwrap_or_default();
                same_crate(crate_name, name) && parts.next().is_none_or(|v| v == version)
            })
    };
    Ok(lock
        .package
        .iter()
        .filter(|package| same_crate(&package.name, name))
        .filter(|package| {
            package
                .source
                .as_deref()
                .is_some_and(|s| s.starts_with("registry+") || s.starts_with("sparse+"))
        })
        .map(|package| Used {
            direct: asked_for(&package.version),
            version: package.version.clone(),
        })
        .collect())
}
