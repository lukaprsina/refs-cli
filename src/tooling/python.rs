//! The Python tools: Black.

use super::Finding;
use super::site::{PRE_COMMIT, PYPROJECT, Site};

/// Black skips what `exclude`, `extend-exclude` or `force-exclude` of `[tool.black]` matches.
/// `pyproject.toml` is shared with other tools, so naming the directory elsewhere in it proves
/// nothing. A pre-commit config is read as text. `.gitignore` covers unless `exclude` replaces
/// the default exclusions.
pub(super) fn black(site: &Site) -> Finding {
    let table = site.pyproject_table("black");
    let hook = site.hook("black");
    if table.is_none() && !hook {
        return Finding::Absent;
    }
    let escaped = escape_regex(&site.name);
    let option = |name: &str| table.and_then(|table| table.get(name));
    let excluded = ["exclude", "extend-exclude", "force-exclude"]
        .iter()
        .filter_map(|name| option(name)?.as_str())
        .any(|regex| regex.contains(&site.name) || regex.contains(&escaped));
    // Without `exclude`, Black also skips what `.gitignore` of its root names (tested: `docs/research/tool-ignore-support.md`).
    let by_gitignore = option("exclude").is_none() && site.lists_dir(".gitignore");
    if excluded || by_gitignore || (hook && site.any_names_dir(&[PRE_COMMIT])) {
        return Finding::Covered;
    }
    // Black takes a regex, anchored at the project root with `^/`. The header is only for a
    // `pyproject.toml` that has no `[tool.black]` yet: a second one is invalid TOML.
    let line = format!("extend-exclude = '^/{}/'", escape_regex(&site.seen));
    let snippet = match table {
        Some(_) => line,
        None => format!("[tool.black]\n{line}"),
    };
    Finding::gap(PYPROJECT, snippet)
}

/// `text` as a regex that matches it literally.
fn escape_regex(text: &str) -> String {
    text.chars()
        .flat_map(|c| match c {
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' => {
                vec!['\\', c]
            }
            _ => vec![c],
        })
        .collect()
}
