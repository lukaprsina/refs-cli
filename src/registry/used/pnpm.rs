//! `pnpm-lock.yaml`. The dependency groups of an importer (or, in v5 and v6, of the top level)
//! are what the project asks for: those versions are `direct`. Every other version in `packages`
//! or `snapshots` is something the project's packages use. The keys there are `name@version`
//! (v6, v9), possibly with a peer suffix, or `/name/version` (v5).

use serde_yaml_ng::Value;

use super::Used;
use super::yaml::{is_registry_version, split_pkg_spec, strip_peer_suffix};

const DEP_GROUPS: [&str; 3] = ["dependencies", "devDependencies", "optionalDependencies"];

pub(super) fn used(pkg: &str, text: &str) -> Result<Vec<Used>, String> {
    let doc: Value = serde_yaml_ng::from_str(text).map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    let mut add = |version: &str, direct: bool| {
        let version = strip_peer_suffix(version);
        if is_registry_version(version) {
            found.push(Used {
                version: version.to_owned(),
                direct,
            });
        }
    };

    let importers = doc.get("importers").and_then(Value::as_mapping);
    let scopes = importers.into_iter().flat_map(|m| m.values());
    for scope in scopes.chain(std::iter::once(&doc)) {
        for group in DEP_GROUPS {
            // A dependency is `name: 1.2.3` (v5) or `name: { specifier, version }` (v6, v9).
            let dep = scope.get(group).and_then(|deps| deps.get(pkg));
            let version = dep.and_then(|dep| dep.as_str().or_else(|| dep.get("version")?.as_str()));
            if let Some(version) = version {
                add(version, true);
            }
        }
    }
    for section in ["packages", "snapshots"] {
        let keys = doc.get(section).and_then(Value::as_mapping);
        for key in keys
            .into_iter()
            .flat_map(|m| m.keys())
            .filter_map(Value::as_str)
        {
            let key = key.strip_prefix('/').unwrap_or(key);
            if let Some((name, version)) = split_pkg_spec(key)
                && name == pkg
            {
                add(version, false);
            }
        }
    }
    Ok(found)
}
