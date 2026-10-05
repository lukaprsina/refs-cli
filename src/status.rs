//! What `refs list --status` knows about each Repo: a row from the Lock and the Checkout
//! state `inspect` finds. `list` only renders the rows.

use std::path::Path;

use crate::active::{LaidRepo, active};
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

/// What `build` found: the Lock and the Checkouts of the active Repos. `row` answers for any
/// Repo, so `list` asks as it walks the config and has no row to pair or to lack.
#[derive(Debug)]
pub struct Status {
    lock: Option<Lock>,
    checkouts: Checkouts,
}

impl Status {
    pub fn row(&self, laid: &LaidRepo) -> Row {
        let id = laid.repo.id;
        let locked = self.lock.as_ref().and_then(|lock| lock.get(id));
        let sha = locked.map(|l| l.pin.short_id().to_string());
        let kind = match (laid.enabled, locked, self.checkouts.get(id)) {
            (false, _, _) => Kind::Disabled,
            (true, Some(locked), Some(observed)) => {
                Kind::Checkout(classify(observed, laid.repo.repo, &locked.pin))
            }
            (true, _, _) => Kind::NotLocked,
        };
        Row { sha, kind }
    }
}

/// Read the Lock and inspect every active Repo, as the plan does, so a broken disabled Repo
/// does not fail it. Every inspect failure is collected.
pub fn build(
    source: &dyn Source,
    root: &Path,
    config: &Config,
) -> Result<Status, Vec<miette::Report>> {
    let lock = Lock::read(&Lock::path(root)).map_err(|e| vec![miette::Report::new(e)])?;
    let checkouts = Checkouts::observe(&active(config), &[], |name| {
        source.inspect(name).map_err(|e| e.for_repo(name))
    })?;
    Ok(Status { lock, checkouts })
}
