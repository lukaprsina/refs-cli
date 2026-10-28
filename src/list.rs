//! `refs list`: every repo in the config, grouped, with its ref, paths and whether it is
//! enabled. With a `Status` (the Lock and the Checkout states) each line also gets the locked
//! SHA and the state of its Checkout. `list` is pure over the two; `status` does the reading.

use crate::active::{LaidRepo, active, layout};
use crate::config::Config;
use crate::lock::Lock;
use crate::plan::{Cause, CheckoutState, Checkouts, classify};
use crate::project::Project;
use crate::source::Source;

/// What `list --status` adds to each row: the Lock and the Checkouts of the active Repos.
#[derive(Debug)]
pub struct Status {
    lock: Option<Lock>,
    checkouts: Checkouts,
}

/// Read the Lock and inspect every active Repo, as the plan does, so a broken disabled Repo
/// does not fail it. Every inspect failure is collected.
pub fn status(source: &dyn Source, project: &Project) -> Result<Status, Vec<miette::Report>> {
    let lock = Lock::read(&Lock::path(&project.root)).map_err(|e| vec![miette::Report::new(e)])?;
    let checkouts = Checkouts::observe(&active(&project.config), &[], |name| {
        source.inspect(name).map_err(|e| e.for_repo(name))
    })?;
    Ok(Status { lock, checkouts })
}

impl Status {
    /// The short locked SHA (`-` when the Lock has no Pin, disabled or not) and the state:
    /// ok, missing, wrong SHA, wrong paths (either also `dirty` when sync would refuse to move
    /// it), foreign, not locked, or disabled.
    fn cells(&self, laid: &LaidRepo) -> [String; 2] {
        let id = laid.repo.id;
        let locked = self.lock.as_ref().and_then(|lock| lock.get(id));
        let sha = locked.map_or("-".into(), |l| l.pin.short_id().to_string());
        let state = match (laid.enabled, locked, self.checkouts.get(id)) {
            (false, _, _) => "disabled".into(),
            (true, Some(locked), Some(observed)) => {
                label(&classify(observed, laid.repo.repo, &locked.pin))
            }
            (true, _, _) => "not locked".into(),
        };
        [sha, state]
    }
}

/// `color` dims the disabled lines with ANSI escapes. With a `status`, each line also gets the
/// locked SHA and the state of its Checkout.
pub fn list(config: &Config, status: Option<&Status>, color: bool) -> String {
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
    ];
    if let Some(status) = status {
        cells.extend(status.cells(r));
    }
    cells
}

/// The label of a Checkout state, with `dirty` added when `sync` would refuse to move the
/// Checkout.
fn label(state: &CheckoutState) -> String {
    match state {
        CheckoutState::InSync => "ok".into(),
        // locked, but nothing (or a dangling directory) is checked out
        CheckoutState::Absent | CheckoutState::Dangling => "missing".into(),
        CheckoutState::Foreign => "foreign".into(),
        CheckoutState::Stale { cause, dirty_files } => {
            let cause = match cause {
                Cause::Commit => "wrong SHA",
                Cause::Paths => "wrong paths",
            };
            let dirty = if dirty_files.is_empty() {
                ""
            } else {
                ", dirty"
            };
            format!("{cause}{dirty}")
        }
    }
}
