//! Reading the package names a Checkout defines from its Manifests (ADR 0009).

use std::collections::BTreeSet;
use std::path::Path;

/// The names of the packages the Manifests in `checkout` define: deduplicated and sorted.
pub fn infer_packages(checkout: &Path) -> Vec<String> {
    let mut names = BTreeSet::new();
    walk(checkout, &mut names);
    names.into_iter().collect()
}

fn walk(dir: &Path, names: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() && !skipped(&entry.file_name().to_string_lossy()) => {
                walk(&path, names);
            }
            Ok(kind) if kind.is_file() => {
                if let Some(read) = reader(&entry.file_name().to_string_lossy()) {
                    names.extend(std::fs::read_to_string(&path).ok().and_then(|t| read(&t)));
                }
            }
            _ => {}
        }
    }
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
fn reader(file: &str) -> Option<fn(&str) -> Option<String>> {
    match file {
        "package.json" => Some(package_json),
        "Cargo.toml" => Some(cargo_toml),
        "pyproject.toml" => Some(pyproject_toml),
        "go.mod" => Some(go_mod),
        _ => None,
    }
}

fn package_json(text: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    if json["private"].as_bool() == Some(true) {
        return None;
    }
    json["name"].as_str().map(str::to_owned)
}

fn cargo_toml(text: &str) -> Option<String> {
    let toml: toml::Table = text.parse().ok()?;
    toml.get("package")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}

fn pyproject_toml(text: &str) -> Option<String> {
    let toml: toml::Table = text.parse().ok()?;
    toml.get("project")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}

/// The path on the `module` line, which may be quoted and followed by a `//` comment.
fn go_mod(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("module")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let path = rest.split("//").next()?.trim().trim_matches(['"', '`']);
        Some(path.to_owned()).filter(|path| !path.is_empty())
    })
}
