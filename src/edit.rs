//! Comment-preserving edits of `refs.toml` (spec §6.3): text in, text out.
//!
//! Each edit parses the text it was given, changes it with `toml_edit`, and validates the
//! result with `config::parse` before returning it, so a rejected edit has produced no text
//! to write.

use toml_edit::{Array, Document, DocumentMut, Item, Table, value};

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

/// Append a repo table to the end of `text`, after a blank line. The text before it is
/// left as it was, so the edit cannot disturb it.
pub fn add(text: &str, req: &AddRepo) -> Result<String, EditError> {
    let config = open(text)?;
    let id = req.id.clone().unwrap_or_else(|| id_from_url(&req.url));
    if config.repos.contains_key(id.as_str()) {
        return Err(EditError::IdTaken { id });
    }
    if parse(text)?.get("repos").is_some_and(Item::is_inline_table) {
        return Err(EditError::Unreadable(
            "`repos` is an inline table; write each repo as a `[repos.<id>]` table".into(),
        ));
    }
    let mut out = text.to_owned();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&repo_table(&id, req));
    match_line_endings(&mut out, text);
    if !text.is_empty() && !text.ends_with('\n') {
        out.pop();
    }
    config::parse(&out)?;
    Ok(out)
}

/// `[repos.<id>]` and its keys, as one block of text ending in a newline.
fn repo_table(id: &str, req: &AddRepo) -> String {
    let mut table = Table::new();
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
    let mut repos = Table::new();
    repos.set_implicit(true);
    repos.insert(id, Item::Table(table));
    let mut doc = DocumentMut::new();
    doc.insert("repos", Item::Table(repos));
    doc.to_string()
}

/// Drop the repo `id` with the comment lines directly above it, and its group when it was
/// the group's last repo (disabled repos count) and the group has no `description`.
pub fn remove(text: &str, id: &str) -> Result<String, EditError> {
    let config = open(text)?;
    let repo = config
        .repos
        .get(id)
        .ok_or_else(|| EditError::UnknownRepo { id: id.into() })?;
    let mut out = cut_table(text, "repos", id)?;
    if let Some(group) = repo.group.as_ref().map(|g| g.as_ref().as_str()) {
        let has_members = config.repos.iter().any(|(other, r)| {
            other.as_ref() != id && r.group.as_ref().is_some_and(|g| g.as_ref() == group)
        });
        let described = config
            .groups
            .get(group)
            .is_some_and(|g| g.description.is_some());
        if !has_members && !described {
            out = cut_table(&out, "groups", group)?;
        }
    }
    config::parse(&out)?;
    Ok(out)
}

/// Cut the table `[<parent>.<key>]` out of `text`: its header, its comment lines directly
/// above, the blank line before those, and every line up to its last key. What follows the
/// last key (blank lines, comments for the next table) stays. Cutting by text, not through
/// the document, leaves every other byte alone.
fn cut_table(text: &str, parent: &str, key: &str) -> Result<String, EditError> {
    let doc = parse(text)?;
    let table = doc
        .get(parent)
        .and_then(|p| p.get(key))
        .and_then(Item::as_table)
        .ok_or_else(|| {
            EditError::Unreadable(format!(
                "`{parent}.{key}` is not a `[{parent}.{key}]` table"
            ))
        })?;
    let header = table.span().map_or(0, |span| span.start);
    let body_end = table
        .iter()
        .filter_map(|(_, item)| item.span())
        .map(|span| span.end)
        .max()
        .unwrap_or_else(|| table.span().map_or(0, |span| span.end));
    let end = text[body_end..]
        .find('\n')
        .map_or(text.len(), |i| body_end + i + 1);

    let lines: Vec<&str> = text[..header].split_inclusive('\n').collect();
    let mut start = header;
    let mut above = lines.iter().rev().peekable();
    while let Some(line) = above.next_if(|l| l.trim_start().starts_with('#')) {
        start -= line.len();
    }
    if let Some(blank) = above.next_if(|l| l.trim().is_empty()) {
        start -= blank.len();
    }

    let mut out = format!("{}{}", &text[..start], &text[end..]);
    // A file that ended without a newline ended with the table's last line.
    if end == text.len() && !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
        if out.ends_with('\r') {
            out.pop();
        }
    }
    Ok(out)
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

fn open(text: &str) -> Result<config::Config, EditError> {
    Ok(config::parse(text)?)
}

fn parse(text: &str) -> Result<Document<&str>, EditError> {
    Document::parse(text).map_err(|e| EditError::Unreadable(e.to_string()))
}

/// Give `text` the `\r\n` line endings of `original` if it used them.
fn match_line_endings(text: &mut String, original: &str) {
    if original.contains("\r\n") {
        *text = text.replace("\r\n", "\n").replace('\n', "\r\n");
    }
}
