//! Reading the package names a Repo defines from its Manifests (ADR 0009, 0011): which of the
//! listed paths to read, and what the text of them says. Pure over the paths and text, so
//! neither needs a Checkout or git.

use std::collections::BTreeSet;

/// The most Manifests `add` reads, shallowest first.
const MAX_MANIFESTS: usize = 200;

/// Root Manifests in the order their description is taken from.
const DESCRIBING: [&str; 3] = ["package.json", "Cargo.toml", "pyproject.toml"];

/// What the Manifests say: the package names, deduplicated and sorted, and the description of
/// the root Manifest.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Inferred {
    pub packages: Vec<String>,
    pub description: Option<String>,
}

/// The Manifests among the outline's `paths` that a Checkout of `chosen` (the Repo's `paths`;
/// empty is the whole repo) will hold, at most `MAX_MANIFESTS` of them, shallowest first, and how
/// many more there were. Under cone mode a Checkout holds the files at the root, the files along
/// the way to a chosen path, and everything under it. (A submodule is listed like a file, but no
/// Manifest is one.)
pub fn manifest_paths(paths: &[String], chosen: &[String]) -> (Vec<String>, usize) {
    let within = |path: &str, dir: &str| {
        path == dir || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
    };
    let mut found: Vec<&String> = paths
        .iter()
        .filter(|path| {
            let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
            reader(file).is_some()
                && !dir.split('/').any(skipped)
                && (chosen.is_empty()
                    || dir.is_empty()
                    || chosen.iter().any(|c| within(dir, c) || within(c, dir)))
        })
        .collect();
    found.sort_by_key(|path| (path.matches('/').count(), *path));
    let left = found.len().saturating_sub(MAX_MANIFESTS);
    found.truncate(MAX_MANIFESTS);
    (found.into_iter().cloned().collect(), left)
}

/// What the Manifests among `files` (path and text) say.
pub fn infer(files: &[(String, String)]) -> Inferred {
    let mut names = BTreeSet::new();
    let mut root = Vec::new();
    for (path, text) in files {
        let file = path.rsplit('/').next().unwrap_or(path);
        let Some(manifest) = reader(file).map(|read| read(text)) else {
            continue;
        };
        names.extend(manifest.name);
        if !path.contains('/') {
            root.push((file, manifest.description));
        }
    }
    let description = DESCRIBING
        .iter()
        .find_map(|file| root.iter().find(|(f, _)| f == file)?.1.clone());
    Inferred {
        packages: names.into_iter().collect(),
        description,
    }
}

/// What one Manifest says. The `name` is absent when it is not a package that can be imported.
#[derive(Default)]
struct Manifest {
    name: Option<String>,
    description: Option<String>,
}

/// Directories that hold dependencies, build output, examples or test fixtures, and dot
/// directories: their Manifests are not packages the Repo documents.
fn skipped(dir: &str) -> bool {
    const SKIPPED: [&str; 8] = [
        "node_modules",
        "target",
        "vendor",
        "dist",
        "examples",
        "fixtures",
        "test",
        "tests",
    ];
    dir.starts_with('.') || SKIPPED.contains(&dir)
}

/// The reader for a Manifest's file name, if it is one.
fn reader(file: &str) -> Option<fn(&str) -> Manifest> {
    match file {
        "package.json" => Some(package_json),
        "Cargo.toml" => Some(cargo_toml),
        "pyproject.toml" => Some(pyproject_toml),
        "go.mod" => Some(go_mod),
        _ => None,
    }
}

/// A description on one line, or `None` when there is nothing in it.
fn one_line(text: Option<&str>) -> Option<String> {
    let line = text?.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(line).filter(|line| !line.is_empty())
}

/// The `name` of a package that can be installed: a `"private": true` one is never imported,
/// and a name npm would refuse is a subpath-export shim (`solid-js/h`), not a package.
fn package_json(text: &str) -> Manifest {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return Manifest::default();
    };
    let public = json["private"].as_bool() != Some(true);
    Manifest {
        name: json["name"]
            .as_str()
            .filter(|name| public && valid_npm_name(name))
            .map(str::to_owned),
        description: one_line(json["description"].as_str()),
    }
}

/// Whether npm accepts `name` for a package that is already published, after
/// `validate-npm-package-name`: its errors, not its warnings (capitals and old names stay).
fn valid_npm_name(name: &str) -> bool {
    let url_friendly = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c))
    };
    let package = match name.strip_prefix('@') {
        Some(scoped) => match scoped.split_once('/') {
            Some((scope, package)) if url_friendly(scope) => package,
            _ => return false,
        },
        None => name,
    };
    url_friendly(package)
        && !name.starts_with(['.', '-', '_'])
        && !package.starts_with('.')
        && !["node_modules", "favicon.ico"].contains(&name.to_lowercase().as_str())
}

/// The `name` of a Cargo package, unless `publish` rules out every registry.
fn cargo_toml(text: &str) -> Manifest {
    let Some(package) = text
        .parse::<toml::Table>()
        .ok()
        .and_then(|toml| toml.get("package").cloned())
    else {
        return Manifest::default();
    };
    let unpublished = match package.get("publish") {
        Some(toml::Value::Boolean(publish)) => !publish,
        Some(toml::Value::Array(registries)) => registries.is_empty(),
        _ => false,
    };
    Manifest {
        name: package
            .get("name")
            .and_then(|name| name.as_str())
            .filter(|_| !unpublished)
            .map(str::to_owned),
        description: one_line(package.get("description").and_then(|d| d.as_str())),
    }
}

fn pyproject_toml(text: &str) -> Manifest {
    let project = text
        .parse::<toml::Table>()
        .ok()
        .and_then(|toml| toml.get("project").cloned());
    let field = |key: &str| project.as_ref()?.get(key)?.as_str().map(str::to_owned);
    Manifest {
        name: field("name"),
        description: one_line(field("description").as_deref()),
    }
}

/// The path on the `module` line, which may be quoted and followed by a `//` comment.
fn go_mod(text: &str) -> Manifest {
    let name = text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("module")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let path = rest.split("//").next()?.trim().trim_matches(['"', '`']);
        Some(path.to_owned()).filter(|path| !path.is_empty())
    });
    Manifest {
        name,
        description: None,
    }
}
