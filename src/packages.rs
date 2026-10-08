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
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() && !skipped(&file_name) => walk(&path, names),
            Ok(kind) if kind.is_file() => {
                if let Some(read) = reader(&file_name) {
                    names.extend(std::fs::read_to_string(&path).ok().and_then(|t| read(&t)));
                }
            }
            _ => {}
        }
    }
}

/// Directories that hold dependencies, build output, examples or test fixtures, and dot
/// directories: their Manifests are not packages the Repo documents. (Private npm Manifests
/// are skipped by `package_json`.)
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

/// The `name` of a package that can be published: a `"private": true` one is never imported.
fn package_json(text: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    if json["private"].as_bool() == Some(true) {
        return None;
    }
    json["name"].as_str().map(str::to_owned)
}

fn cargo_toml(text: &str) -> Option<String> {
    toml_name(text, "package")
}

fn pyproject_toml(text: &str) -> Option<String> {
    toml_name(text, "project")
}

/// The `name` in the `[<table>]` of a TOML Manifest.
fn toml_name(text: &str, table: &str) -> Option<String> {
    let toml: toml::Table = text.parse().ok()?;
    toml.get(table)?.get("name")?.as_str().map(str::to_owned)
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
