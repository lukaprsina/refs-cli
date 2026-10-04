//! `refs list`: every repo in the config, grouped, with its ref, paths and whether it is
//! enabled. Pure: `list` reads the config only; `list_status` also takes the Lock and what is
//! on disk, which `sync::status` collects. Both walk `active::layout`.

use std::collections::HashMap;

use crate::active::{LaidRepo, layout};
use crate::config::Config;
use crate::lock::Lock;
use crate::source::Observed;

/// What `refs list --status` adds to the config: the Lock and what is on disk, by repo id
/// (an id with no entry counts as absent).
pub struct Status {
    pub lock: Option<Lock>,
    pub observed: HashMap<String, Observed>,
}

/// `color` dims the disabled lines with ANSI escapes.
pub fn list(config: &Config, color: bool) -> String {
    render(config, None, color)
}

/// `list` with the locked SHA (short) and the state of each checkout: ok, missing, wrong
/// SHA, foreign, not locked, or disabled.
pub fn list_status(config: &Config, status: &Status, color: bool) -> String {
    render(config, Some(status), color)
}

fn render(config: &Config, status: Option<&Status>, color: bool) -> String {
    let sections = layout(config);
    let has_groups = !config.groups.is_empty();
    // One row of cells per repo; the columns are aligned across the whole listing.
    let rows: Vec<Vec<Vec<String>>> = sections
        .iter()
        .map(|s| s.repos.iter().map(|r| cells(r, status)).collect())
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
                let name = g.group.title(g.id);
                let mut heading = if name == g.id {
                    name.to_string()
                } else {
                    format!("{name} ({})", g.id)
                };
                if !g.enabled {
                    heading.push_str("  off");
                }
                out.push_str(&dim(heading, g.enabled));
                out.push('\n');
            }
            None if has_groups => out.push_str("ungrouped\n"),
            None => {}
        }
        for (repo, row) in section.repos.iter().zip(rows) {
            let last = row.len() - 1;
            let mut line = String::from("  ");
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    line.push_str("  ");
                }
                line.push_str(cell);
                if i < last {
                    line.extend(std::iter::repeat_n(' ', widths[i] - cell.chars().count()));
                }
            }
            out.push_str(&dim(line, repo.enabled));
            out.push('\n');
        }
    }
    out
}

/// The cells of one repo's line: id, url, ref, paths, on/off, and with `--status` the short
/// locked SHA and the checkout state.
fn cells(r: &LaidRepo, status: Option<&Status>) -> Vec<String> {
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
        if r.enabled { "on" } else { "off" }.to_string(),
    ];
    if let Some(status) = status {
        let sha = status
            .lock
            .as_ref()
            .and_then(|lock| lock.get(r.repo.id))
            .map_or("-", |l| l.pin.short_id());
        cells.push(sha.to_string());
        cells.push(checkout_state(status, r).to_string());
    }
    cells
}

/// Where one repo stands against the Lock and the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutState {
    Ok,
    /// Locked, but nothing (or a dangling directory) is checked out.
    Missing,
    WrongSha,
    /// The right commit, checked out with other `paths` than the config asks for.
    WrongPaths,
    /// A directory that is not one of ours is in the way.
    Foreign,
    NotLocked,
    Disabled,
}

impl std::fmt::Display for CheckoutState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            CheckoutState::Ok => "ok",
            CheckoutState::Missing => "missing",
            CheckoutState::WrongSha => "wrong SHA",
            CheckoutState::WrongPaths => "wrong paths",
            CheckoutState::Foreign => "foreign",
            CheckoutState::NotLocked => "not locked",
            CheckoutState::Disabled => "disabled",
        })
    }
}

fn checkout_state(status: &Status, laid: &LaidRepo) -> CheckoutState {
    let r = laid.repo;
    if !laid.enabled {
        return CheckoutState::Disabled;
    }
    let Some(locked) = status.lock.as_ref().and_then(|lock| lock.get(r.id)) else {
        return CheckoutState::NotLocked;
    };
    match status.observed.get(r.id) {
        None | Some(Observed::Absent | Observed::Dangling) => CheckoutState::Missing,
        Some(Observed::Foreign) => CheckoutState::Foreign,
        Some(seen) if seen.matches(r.repo, &locked.pin) => CheckoutState::Ok,
        Some(Observed::At { pin, .. }) if !pin.same_commit(&locked.pin) => CheckoutState::WrongSha,
        Some(Observed::At { .. }) => CheckoutState::WrongPaths,
    }
}
