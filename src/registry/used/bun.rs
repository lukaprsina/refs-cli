//! `bun.lock`: JSONC, which here means JSON with trailing commas. `workspaces` maps each
//! workspace's path to its `name` and dependency groups, which are what the project asks for;
//! `packages` maps an install path (`ms`, or `lib/ms` for a copy nested under `lib`) to
//! `["<name>@<version>", registry, { dependencies }, integrity]`. The name is the one the
//! package really has, whatever alias it is installed under.

use serde_json::Value;

use super::npm_spec::{is_registry_version, split_pkg_spec};
use super::{DEPENDENCY_GROUPS, Used};

/// `text` without the commas that precede a closing `}` or `]`, which `serde_json` refuses.
/// Commas inside strings stay.
fn without_trailing_commas(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut in_string, mut escaped) = (false, false);
    for (at, c) in text.char_indices() {
        if in_string {
            in_string = escaped || c != '"';
            escaped = !escaped && c == '\\';
        } else if c == ',' && text[at + 1..].trim_start().starts_with(['}', ']']) {
            continue;
        } else {
            in_string = c == '"';
        }
        out.push(c);
    }
    out
}

/// Every `(workspace name, alias)` that a workspace's dependency groups name.
fn asked_for(doc: &Value) -> Vec<(&str, &str)> {
    let Some(workspaces) = doc.get("workspaces").and_then(Value::as_object) else {
        return Vec::new();
    };
    workspaces
        .values()
        .flat_map(|workspace| {
            let owner = workspace.get("name").and_then(Value::as_str).unwrap_or("");
            DEPENDENCY_GROUPS
                .iter()
                .filter_map(|group| workspace.get(group)?.as_object())
                .flat_map(|deps| deps.keys())
                .map(move |alias| (owner, alias.as_str()))
        })
        .collect()
}

pub(crate) fn used(name: &str, text: &str) -> Result<Vec<Used>, String> {
    let doc: Value =
        serde_json::from_str(&without_trailing_commas(text)).map_err(|e| e.to_string())?;
    let asked_for = asked_for(&doc);
    // The copy at the top, or the one nested under the workspace that asks for it.
    let direct = |path: &str| {
        asked_for
            .iter()
            .any(|(owner, alias)| path == *alias || path == format!("{owner}/{alias}"))
    };
    let Some(packages) = doc.get("packages").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    Ok(packages
        .iter()
        .filter_map(|(path, entry)| {
            let spec = entry.get(0)?.as_str()?;
            let (real_name, version) = split_pkg_spec(spec)?;
            (real_name == name && is_registry_version(version)).then(|| Used {
                version: version.to_owned(),
                direct: direct(path),
            })
        })
        .collect())
}
