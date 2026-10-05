//! What `refs list --status` knows about each Repo: one row per Repo in the config, built
//! from the Lock and the Checkout state `inspect` finds. `list` only renders the rows.

use std::collections::HashMap;
use std::path::Path;

use crate::active::{active, layout};
use crate::config::Config;
use crate::lock::Lock;
use crate::plan::{CheckoutState, Checkouts, classify};
use crate::source::Source;

/// How one Repo stands: decided before there is a Checkout state (`Disabled`, `NotLocked`),
/// or the Checkout state of an active, locked Repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Disabled,
    NotLocked,
    Checkout(CheckoutState),
}

/// One Repo's row: the short locked SHA (when the Lock has a Pin for it, disabled or not)
/// and its `Kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub sha: Option<String>,
    pub kind: Kind,
}

/// The rows of every Repo in the config, by repo id.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub rows: HashMap<String, Row>,
}

/// One row per Repo. Only active Repos are inspected, as the plan does, so a broken disabled
/// Repo does not fail it. Every inspect failure is collected.
pub fn build(
    source: &dyn Source,
    root: &Path,
    config: &Config,
) -> Result<Status, Vec<miette::Report>> {
    let lock = Lock::read(&Lock::path(root)).map_err(|e| vec![miette::Report::new(e)])?;
    let set = active(config);
    let checkouts = Checkouts::observe_active(&set, |name| {
        source.inspect(name).map_err(|e| e.for_repo(name))
    })?;
    let rows = layout(config)
        .iter()
        .flat_map(|section| &section.repos)
        .map(|laid| {
            let id = laid.repo.id;
            let locked = lock.as_ref().and_then(|lock| lock.get(id));
            let sha = locked.map(|l| l.pin.short_id().to_string());
            let kind = match (laid.enabled, locked, checkouts.get(id)) {
                (false, _, _) => Kind::Disabled,
                (true, Some(locked), Some(observed)) => {
                    Kind::Checkout(classify(observed, laid.repo.repo, &locked.pin))
                }
                (true, _, _) => Kind::NotLocked,
            };
            (id.to_string(), Row { sha, kind })
        })
        .collect();
    Ok(Status { rows })
}
