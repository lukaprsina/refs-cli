//! `refs list`: every repo in the config, grouped, with its ref, paths and whether it is
//! enabled. Pure: `list` reads the config only; `list_status` also asks for the row of each
//! Repo (from `status`), and only renders it. Both walk `active::layout`.

use crate::active::{LaidRepo, layout};
use crate::config::Config;
use crate::plan::{Cause, CheckoutState};
use crate::status::{Kind, Row};

/// `color` dims the disabled lines with ANSI escapes.
pub fn list(config: &Config, color: bool) -> String {
    render(config, None, color)
}

/// `list` with the locked SHA (short) and the state of each checkout: ok, missing, wrong
/// SHA, wrong paths (either also `dirty` when sync would refuse to move it), foreign, not
/// locked, or disabled. `row_of` gives the row of each Repo as the listing walks the config.
pub fn list_status(config: &Config, row_of: impl Fn(&LaidRepo) -> Row, color: bool) -> String {
    render(config, Some(&row_of), color)
}

fn render(config: &Config, row_of: Option<&dyn Fn(&LaidRepo) -> Row>, color: bool) -> String {
    let sections = layout(config);
    let has_groups = !config.groups.is_empty();
    // One row of cells per repo; the columns are aligned across the whole listing.
    let rows: Vec<Vec<Vec<String>>> = sections
        .iter()
        .map(|s| {
            s.repos
                .iter()
                .map(|r| cells(r, row_of.map(|row_of| row_of(r))))
                .collect()
        })
        .collect();
    let columns = rows.iter().flatten().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|i| {
            rows.iter()
                .flatten()
                .filter_map(|row| row.get(i))
                .map(|c| c.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let dim = |text: String, enabled: bool| {
        if color && !enabled {
            format!("\x1b[2m{text}\x1b[0m")
        } else {
            text
        }
    };
    let mut out = String::new();
    for (section, rows) in sections.iter().zip(&rows) {
        match &section.group {
            Some(g) => {
                let heading = g.group.listed_as(g.id);
                out.push_str(&dim(marked(&heading, g.enabled, ""), g.enabled));
                out.push('\n');
            }
            None if has_groups => out.push_str("ungrouped\n"),
            None => {}
        }
        for (repo, row) in section.repos.iter().zip(rows) {
            let last = row.len() - 1;
            let mut line = String::new();
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    line.push_str("  ");
                }
                line.push_str(cell);
                if i < last {
                    line.extend(std::iter::repeat_n(' ', widths[i] - cell.chars().count()));
                }
            }
            out.push_str(&dim(marked(&line, repo.enabled, "  "), repo.enabled));
            out.push('\n');
        }
    }
    out
}

/// `line` led by the disabled marker `- `, which survives where colour does not (a pipe,
/// `NO_COLOR`); an enabled line gets `indent` instead.
fn marked(line: &str, enabled: bool, indent: &str) -> String {
    format!("{}{line}", if enabled { indent } else { "- " })
}

/// The cells of one repo's line: id, url, ref, paths, and with `--status` the short
/// locked SHA and the checkout state.
fn cells(r: &LaidRepo, row: Option<Row>) -> Vec<String> {
    let repo = r.repo.repo;
    let paths = if repo.paths.is_empty() {
        "(whole repo)".to_string()
    } else {
        repo.path_strings().join(", ")
    };
    let mut cells = vec![
        r.repo.id.to_string(),
        repo.url.as_ref().to_string(),
        repo.effective_ref().to_string(),
        paths,
    ];
    if let Some(row) = row {
        cells.push(row.sha.clone().unwrap_or("-".into()));
        cells.push(label(&row).to_string());
    }
    cells
}

/// How one repo's Checkout is labelled: its Checkout state, plus the two states `list`
/// decides before there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLabel {
    Ok,
    /// Locked, but nothing (or a dangling directory) is checked out.
    Missing,
    /// Another commit is checked out; `dirty` when sync would refuse to move it.
    WrongSha {
        dirty: bool,
    },
    /// The right commit, checked out with other `paths` than the config asks for (a set, so
    /// order does not count); `sync` checks it out again unless it is `dirty`.
    WrongPaths {
        dirty: bool,
    },
    /// A directory that is not one of ours is in the way.
    Foreign,
    NotLocked,
    Disabled,
}

impl std::fmt::Display for StatusLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let (text, dirty) = match self {
            StatusLabel::Ok => ("ok", false),
            StatusLabel::Missing => ("missing", false),
            StatusLabel::WrongSha { dirty } => ("wrong SHA", *dirty),
            StatusLabel::WrongPaths { dirty } => ("wrong paths", *dirty),
            StatusLabel::Foreign => ("foreign", false),
            StatusLabel::NotLocked => ("not locked", false),
            StatusLabel::Disabled => ("disabled", false),
        };
        f.write_str(text)?;
        if dirty {
            f.write_str(", dirty")?;
        }
        Ok(())
    }
}

/// How a row's Repo is labelled.
fn label(row: &Row) -> StatusLabel {
    match &row.kind {
        Kind::NotLocked => StatusLabel::NotLocked,
        Kind::Disabled => StatusLabel::Disabled,
        Kind::Checkout(state) => match state {
            CheckoutState::InSync => StatusLabel::Ok,
            CheckoutState::Absent | CheckoutState::Dangling => StatusLabel::Missing,
            CheckoutState::Foreign => StatusLabel::Foreign,
            CheckoutState::Stale { cause, dirty_files } => {
                let dirty = !dirty_files.is_empty();
                match cause {
                    Cause::Commit => StatusLabel::WrongSha { dirty },
                    Cause::Paths => StatusLabel::WrongPaths { dirty },
                }
            }
        },
    }
}
