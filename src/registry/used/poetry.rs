//! `poetry.lock`: `[[package]]` tables. A package with no `[package.source]` comes from PyPI;
//! one with a source (`git`, `directory`, `url`, or `legacy` for another index) does not. The
//! lockfile does not list what the project asks for (`groups` also holds for dependencies of
//! dependencies), so no version is direct.

use serde::Deserialize;

use super::Used;
use super::python::same_project;

#[derive(Deserialize)]
struct Lock {
    #[serde(default)]
    package: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    source: Option<toml::Value>,
}

pub(crate) fn used(name: &str, text: &str) -> Result<Vec<Used>, String> {
    let lock: Lock = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(lock
        .package
        .into_iter()
        .filter(|package| package.source.is_none() && same_project(&package.name, name))
        .map(|package| Used {
            version: package.version,
            direct: false,
        })
        .collect())
}
