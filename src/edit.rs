//! Comment-preserving edits of `refs.toml` (spec §6.3): text in, text out.
//!
//! Each edit parses the text it was given, changes it with `toml_edit`, and validates the
//! result with `config::parse` before returning it, so a rejected edit has produced no text
//! to write.

use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::config;
use crate::diagnostic::EditError;

/// What `refs add` was asked for. Only `url` is required; the rest follow spec §6.1.
#[derive(Debug, Default)]
pub struct AddRepo {
    pub url: String,
    pub id: Option<String>,
    pub group: Option<String>,
    pub git_ref: Option<String>,
    pub description: Option<String>,
    pub paths: Vec<String>,
    pub packages: Vec<String>,
    pub start: Vec<String>,
}

/// Append a repo table to the end of `text`.
pub fn add(text: &str, req: &AddRepo) -> Result<String, EditError> {
    let mut doc = open(text)?;
    let id = req.id.clone().unwrap_or_else(|| id_from_url(&req.url));
    let repos = doc
        .entry("repos")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| EditError::Unreadable("`repos` is not a table".into()))?;
    repos.set_implicit(true);
    if repos.contains_key(&id) {
        return Err(EditError::IdTaken { id });
    }
    let mut table = Table::new();
    table.decor_mut().set_prefix("\n");
    table["url"] = value(&req.url);
    if let Some(group) = &req.group {
        table["group"] = value(group);
    }
    if let Some(git_ref) = &req.git_ref {
        table["ref"] = value(git_ref);
    }
    if let Some(description) = &req.description {
        table["description"] = value(description);
    }
    for (key, items) in [
        ("paths", &req.paths),
        ("packages", &req.packages),
        ("start", &req.start),
    ] {
        if !items.is_empty() {
            table[key] = value(items.iter().collect::<Array>());
        }
    }
    repos.insert(&id, Item::Table(table));
    finish(doc, text)
}

/// Drop the repo `id`, with the comments attached to its table, and its group when it was
/// the group's last repo (disabled repos count) and the group has no `description`.
pub fn remove(text: &str, id: &str) -> Result<String, EditError> {
    let mut doc = open(text)?;
    let group = doc
        .get("repos")
        .and_then(|repos| repos.get(id))
        .and_then(Item::as_table_like)
        .ok_or_else(|| EditError::UnknownRepo { id: id.into() })?
        .get("group")
        .and_then(Item::as_str)
        .map(str::to_owned);
    doc["repos"].as_table_mut().map(|repos| repos.remove(id));
    if let Some(group) = group {
        let has_members = doc["repos"].as_table().is_some_and(|repos| {
            repos
                .iter()
                .any(|(_, repo)| repo.get("group").and_then(Item::as_str) == Some(&group))
        });
        let described = doc
            .get("groups")
            .and_then(|groups| groups.get(&group))
            .is_some_and(|g| g.get("description").is_some());
        if !has_members && !described {
            doc["groups"]
                .as_table_mut()
                .map(|groups| groups.remove(&group));
        }
    }
    finish(doc, text)
}

/// The last path segment of the URL, without a trailing `.git`.
fn id_from_url(url: &str) -> String {
    let last = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

fn open(text: &str) -> Result<DocumentMut, EditError> {
    config::parse(text)?;
    text.parse()
        .map_err(|e: toml_edit::TomlError| EditError::Unreadable(e.to_string()))
}

/// Serialise `doc` and validate it. `toml_edit` ends every document with a newline, so a
/// file that had none (`original`) is given back without one.
fn finish(doc: DocumentMut, original: &str) -> Result<String, EditError> {
    let mut text = doc.to_string();
    if !original.is_empty() && !original.ends_with('\n') && text.ends_with('\n') {
        text.pop();
    }
    config::parse(&text)?;
    Ok(text)
}
