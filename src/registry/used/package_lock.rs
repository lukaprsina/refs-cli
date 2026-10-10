//! `package-lock.json`. Version 2 and 3 list every install in `packages`, keyed by path
//! (`node_modules/a/node_modules/b`); the entry `""` is the project, and its dependency groups
//! are what the project asks for. Version 1 has only the nested `dependencies` tree, which does
//! not tell the project's own dependencies from the ones hoisted to the top, so none is direct.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::Used;

/// The dependency groups of the project's own `package.json`.
const GROUPS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

#[derive(Deserialize)]
struct Entry {
    /// The real name, when the install is under an alias.
    name: Option<String>,
    version: Option<String>,
    resolved: Option<String>,
    #[serde(default)]
    link: bool,
}

impl Entry {
    /// The version, if this install came from the registry. Version 1 has no `resolved` for a
    /// git or file install and puts its spec in `version` (`github:me/x#abc`), which no
    /// registry version contains a `:` of.
    fn registry_version(&self) -> Option<&str> {
        let from_registry = !self.link
            && self
                .resolved
                .as_deref()
                .is_none_or(|url| url.starts_with("https://") || url.starts_with("http://"));
        self.version
            .as_deref()
            .filter(|version| from_registry && !version.contains(':'))
    }
}

pub(super) fn used(name: &str, text: &str) -> Result<Vec<Used>, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match doc.get("packages").and_then(Value::as_object) {
        Some(packages) => Ok(from_packages(name, packages)),
        None => Ok(doc
            .get("dependencies")
            .and_then(Value::as_object)
            .map(|tree| from_tree(name, tree))
            .unwrap_or_default()),
    }
}

fn from_packages(name: &str, packages: &Map<String, Value>) -> Vec<Used> {
    let asked_for = |alias: &str| {
        packages.get("").is_some_and(|root| {
            GROUPS.iter().any(|group| {
                root.get(group)
                    .is_some_and(|deps| deps.get(alias).is_some())
            })
        })
    };
    packages
        .iter()
        .filter_map(|(path, value)| {
            // The part after the last `node_modules/` is the name the install is under.
            let (_, alias) = path.rsplit_once("node_modules/")?;
            let entry = Entry::deserialize(value).ok()?;
            if entry.name.as_deref().unwrap_or(alias) != name {
                return None;
            }
            Some(Used {
                version: entry.registry_version()?.to_owned(),
                direct: *path == format!("node_modules/{alias}") && asked_for(alias),
            })
        })
        .collect()
}

/// Version 1: every version in the tree, none of them direct.
fn from_tree(name: &str, tree: &Map<String, Value>) -> Vec<Used> {
    let mut found = Vec::new();
    for (alias, value) in tree {
        if let Ok(entry) = Entry::deserialize(value)
            && entry.name.as_deref().unwrap_or(alias) == name
            && let Some(version) = entry.registry_version()
        {
            found.push(Used {
                version: version.to_owned(),
                direct: false,
            });
        }
        if let Some(nested) = value.get("dependencies").and_then(Value::as_object) {
            found.extend(from_tree(name, nested));
        }
    }
    found
}
