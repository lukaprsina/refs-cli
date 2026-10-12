//! `uv.lock`: `[[package]]` tables. A package whose `source` is `{ registry = "<PyPI>" }` is a
//! PyPI version; one from another index, git, a path or a directory is not. The project itself
//! (the root and the other workspace members) has a `virtual` or `editable` source, and the
//! dependencies it lists, in any group, are what the project asks for.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::Used;
use super::python::{is_pypi_index, same_project};

#[derive(Deserialize)]
struct Lock {
    #[serde(default)]
    package: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    /// Absent for a project that has none.
    version: Option<String>,
    source: Option<Source>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: BTreeMap<String, Vec<Dependency>>,
    #[serde(default, rename = "optional-dependencies")]
    optional_dependencies: BTreeMap<String, Vec<Dependency>>,
}

#[derive(Deserialize)]
struct Source {
    registry: Option<String>,
    r#virtual: Option<String>,
    editable: Option<String>,
}

/// A dependency is `{ name }`, with the `version` and `source` too when the lockfile has
/// several versions of it.
#[derive(Deserialize)]
struct Dependency {
    name: String,
    version: Option<String>,
    source: Option<Source>,
}

impl Dependency {
    /// Whether this is the PyPI `version` of `name`: a dependency that names a source names the
    /// package from that index.
    fn is(&self, name: &str, version: &str) -> bool {
        same_project(&self.name, name)
            && self.version.as_deref().is_none_or(|v| v == version)
            && self
                .source
                .as_ref()
                .is_none_or(|source| source.registry.as_deref().is_some_and(is_pypi_index))
    }
}

impl Package {
    fn is_project(&self) -> bool {
        self.source
            .as_ref()
            .is_some_and(|source| source.r#virtual.is_some() || source.editable.is_some())
    }

    fn asks_for(&self, name: &str, version: &str) -> bool {
        let groups = self
            .dev_dependencies
            .values()
            .chain(self.optional_dependencies.values());
        std::iter::once(&self.dependencies)
            .chain(groups)
            .flatten()
            .any(|dependency| dependency.is(name, version))
    }
}

pub(crate) fn used(name: &str, text: &str) -> Result<Vec<Used>, String> {
    let lock: Lock = toml::from_str(text).map_err(|e| e.to_string())?;
    let asked_for = |version: &str| {
        lock.package
            .iter()
            .filter(|package| package.is_project())
            .any(|package| package.asks_for(name, version))
    };
    Ok(lock
        .package
        .iter()
        .filter(|package| same_project(&package.name, name))
        .filter(|package| {
            package
                .source
                .as_ref()
                .and_then(|source| source.registry.as_deref())
                .is_some_and(is_pypi_index)
        })
        .filter_map(|package| package.version.as_deref())
        .map(|version| Used {
            direct: asked_for(version),
            version: version.to_owned(),
        })
        .collect())
}
