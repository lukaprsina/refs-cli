//! Comment-preserving edits of `refs.toml` (spec §6.3): text in, text out.
//!
//! Each edit parses the text it was given, changes it with `toml_edit`, and validates the
//! result with `config::parse` before returning it, so a rejected edit has produced no text
//! to write.

use toml_edit::{Array, Document, DocumentMut, Item, Table, value};

use crate::config;
use crate::diagnostic::EditError;

/// What `refs add` was asked for. Only `url` is required; the rest follow spec §6.1. It is
/// also the shape of the command line, so the CLI parses straight into it.
#[derive(Debug, Default, clap::Args)]
pub struct AddRepo {
    /// The repository to add
    pub url: String,
    /// The repo's id, instead of the last segment of the URL
    #[arg(long)]
    pub id: Option<String>,
    /// The group to put it in; it must exist
    #[arg(long)]
    pub group: Option<String>,
    /// A branch, tag or full commit id; the remote's default branch when absent
    #[arg(long = "ref", value_name = "REF")]
    pub git_ref: Option<String>,
    /// One line on what the repo is
    #[arg(long)]
    pub description: Option<String>,
    /// Repo-relative directories to check out
    #[arg(long, num_args = 1.., value_name = "PATH")]
    pub paths: Vec<String>,
    /// Names, as imported in code, of the packages the repo documents or implements
    #[arg(long, num_args = 1.., value_name = "PACKAGE")]
    pub packages: Vec<String>,
    /// Repo-relative files worth reading first
    #[arg(long, num_args = 1.., value_name = "PATH")]
    pub start: Vec<String>,
}

/// One edit of `refs.toml`.
#[derive(Debug)]
pub enum Edit<'a> {
    Add(&'a AddRepo),
    Remove(&'a str),
    Disable(Target<'a>),
    Enable(Target<'a>),
}

/// What an edit gave: the new text, and the Group it had to create for it.
#[derive(Debug)]
pub struct Applied {
    pub text: String,
    pub group_created: Option<String>,
}

impl Applied {
    fn of(text: String) -> Applied {
        Applied {
            text,
            group_created: None,
        }
    }
}

impl Edit<'_> {
    /// The text this edit gives `text`.
    pub fn apply(&self, text: &str) -> Result<Applied, EditError> {
        match self {
            Edit::Add(req) => add_creating_group(text, req),
            Edit::Remove(id) => remove(text, id).map(Applied::of),
            Edit::Disable(target) => disable(text, *target).map(Applied::of),
            Edit::Enable(target) => enable(text, *target).map(Applied::of),
        }
    }
}

/// Append a repo table to the end of `text`, after a blank line. The text before it is
/// left as it was, so the edit cannot disturb it. A `--group` that is not in the config is
/// created, bare, just before the repo.
pub fn add(text: &str, req: &AddRepo) -> Result<String, EditError> {
    Ok(add_creating_group(text, req)?.text)
}

fn add_creating_group(text: &str, req: &AddRepo) -> Result<Applied, EditError> {
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
    let group_created = req
        .group
        .as_ref()
        .filter(|group| !config.groups.contains_key(group.as_str()))
        .cloned();
    if group_created.is_some()
        && parse(text)?
            .get("groups")
            .is_some_and(Item::is_inline_table)
    {
        return Err(EditError::Unreadable(
            "`groups` is an inline table; write each group as a `[groups.<id>]` table".into(),
        ));
    }
    let mut out = text.to_owned();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    if let Some(group) = &group_created {
        out.push_str(&table_text("groups", group, Table::new()));
        out.push('\n');
    }
    out.push_str(&table_text("repos", &id, repo_table(req)));
    match_line_endings(&mut out, text);
    if !text.is_empty() && !text.ends_with('\n') {
        out.pop();
    }
    config::parse(&out)?;
    Ok(Applied {
        text: out,
        group_created,
    })
}

/// The keys of a repo table for `req`.
fn repo_table(req: &AddRepo) -> Table {
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
    table
}

/// `[<parent>.<id>]` and its keys, as one block of text ending in a newline.
fn table_text(parent: &str, id: &str, table: Table) -> String {
    let mut parent_table = Table::new();
    parent_table.set_implicit(true);
    parent_table.insert(id, Item::Table(table));
    let mut doc = DocumentMut::new();
    doc.insert(parent, Item::Table(parent_table));
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
    let mut out = cut_table(text, Target::Repo(id))?;
    if let Some(group) = repo.group.as_ref().map(|g| g.as_ref().as_str()) {
        let has_members = config.repos.iter().any(|(other, r)| {
            other.as_ref() != id && r.group.as_ref().is_some_and(|g| g.as_ref() == group)
        });
        let described = config
            .groups
            .get(group)
            .is_some_and(|g| g.description.is_some());
        if !has_members && !described {
            out = cut_table(&out, Target::Group(group))?;
        }
    }
    config::parse(&out)?;
    Ok(out)
}

/// A repo or a group: what `enable`, `disable` and the cuts act on.
#[derive(Debug, Clone, Copy)]
pub enum Target<'a> {
    Repo(&'a str),
    Group(&'a str),
}

impl<'a> Target<'a> {
    /// The table's name in `refs.toml`: `[repos.<id>]` or `[groups.<id>]`.
    fn parent(self) -> &'static str {
        match self {
            Target::Repo(_) => "repos",
            Target::Group(_) => "groups",
        }
    }

    fn id(self) -> &'a str {
        match self {
            Target::Repo(id) | Target::Group(id) => id,
        }
    }
}

/// Set `enabled = false` on the repo or group, or on its existing `enabled` key.
pub fn disable(text: &str, target: Target) -> Result<String, EditError> {
    check_exists(text, target)?;
    let doc = parse(text)?;
    let table = table_of(&doc, target)?;
    let mut out = text.to_owned();
    if let Some(span) = table.get("enabled").and_then(Item::span) {
        out.replace_range(span, "false");
    } else {
        let at = line_end(text, body_end(table));
        let eol = eol(text);
        let line = if text[..at].ends_with('\n') {
            format!("enabled = false{eol}")
        } else {
            format!("{eol}enabled = false")
        };
        out.insert_str(at, &line);
    }
    config::parse(&out)?;
    Ok(out)
}

/// Remove the `enabled` key of the repo or group (spec §6.4: absent means enabled).
pub fn enable(text: &str, target: Target) -> Result<String, EditError> {
    check_exists(text, target)?;
    let doc = parse(text)?;
    let table = table_of(&doc, target)?;
    let Some(span) = table.get("enabled").and_then(Item::span) else {
        return Ok(text.to_owned());
    };
    let key_start = table
        .key("enabled")
        .and_then(toml_edit::Key::span)
        .map_or(span.start, |k| k.start);
    let start = text[..key_start].rfind('\n').map_or(0, |i| i + 1);
    let end = line_end(text, span.end);
    let mut out = format!("{}{}", &text[..start], &text[end..]);
    // The line was the file's last and had no newline: take the one before it too.
    if end == text.len() && !text.ends_with('\n') && start > 0 {
        out.pop();
        if out.ends_with('\r') {
            out.pop();
        }
    }
    config::parse(&out)?;
    Ok(out)
}

/// `target` must be a repo or group of the config in `text`.
fn check_exists(text: &str, target: Target) -> Result<(), EditError> {
    let config = open(text)?;
    match target {
        Target::Repo(id) if !config.repos.contains_key(id) => {
            Err(EditError::UnknownRepo { id: id.into() })
        }
        Target::Group(id) if !config.groups.contains_key(id) => {
            Err(EditError::UnknownGroup { id: id.into() })
        }
        _ => Ok(()),
    }
}

/// The `[repos.<id>]` or `[groups.<id>]` table. Another spelling of the same data (an inline
/// table, dotted keys) is not edited by text.
fn table_of<'d>(doc: &'d Document<&str>, target: Target) -> Result<&'d Table, EditError> {
    let (parent, id) = (target.parent(), target.id());
    doc.get(parent)
        .and_then(|p| p.get(id))
        .and_then(Item::as_table)
        .ok_or_else(|| {
            EditError::Unreadable(format!(
                "`{parent}.{id}` is not written as a `[{parent}.{id}]` table; rewrite it as one"
            ))
        })
}

/// Where the last key of `table` ends.
fn body_end(table: &Table) -> usize {
    table
        .iter()
        .filter_map(|(_, item)| item.span())
        .map(|span| span.end)
        .max()
        .unwrap_or_else(|| table.span().map_or(0, |span| span.end))
}

/// The offset just past the line that `at` is on.
fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i + 1)
}

/// The line ending `text` uses.
fn eol(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Cut the table `target` out of `text`: its header, its comment lines directly
/// above, the blank line before those, and every line up to its last key. What follows the
/// last key (blank lines, comments for the next table) stays. Cutting by text, not through
/// the document, leaves every other byte alone.
fn cut_table(text: &str, target: Target) -> Result<String, EditError> {
    let doc = parse(text)?;
    let table = table_of(&doc, target)?;
    let header = table.span().map_or(0, |span| span.start);
    let end = line_end(text, body_end(table));

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
    if eol(original) == "\r\n" {
        *text = text.replace("\r\n", "\n").replace('\n', "\r\n");
    }
}
