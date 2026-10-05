//! Between the two stages (ADR 0006): what stage 1's failures hold back. The executor has
//! resolved and verified; `settle` decides, as data, which Lock to write, which active Repos
//! the Lock covers, and whether the edit may be written.

use crate::active::ActiveSet;
use crate::lock::{Lock, LockedRepo};

/// What stage 1 does with the Repos that did not resolve or verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Nothing is written unless every Repo passed.
    AllOrNothing,
    /// The entries of the Repos that passed are written (an edit that only shrinks the
    /// active set must not be blocked by a Repo it did not add).
    Passing,
}

/// A Repo that failed to resolve or verify, with the diagnostic for it.
#[derive(Debug)]
pub struct Failure {
    pub id: String,
    pub error: miette::Report,
}

/// The active Repos stage 2 plans for (**Covered**: the Lock has a Pin for them) and the ids
/// stage 1 could not lock (**Withheld**), whose Checkouts stage 2 leaves alone.
#[derive(Debug, Clone)]
pub struct Coverage<'a> {
    pub active: ActiveSet<'a>,
    pub withheld: Vec<String>,
}

impl Failure {
    /// `error` as it happened to the Repo `id`.
    pub fn new(id: &str, error: crate::diagnostic::SourceError) -> Failure {
        Failure {
            id: id.into(),
            error: error.for_repo(id),
        }
    }
}

impl<'a> Coverage<'a> {
    /// Every active Repo is covered and nothing is withheld.
    pub fn full(active: ActiveSet<'a>) -> Coverage<'a> {
        Coverage {
            active,
            withheld: Vec::new(),
        }
    }
}

/// Stage 1's decisions.
#[derive(Debug)]
pub struct Settled<'a> {
    /// The Lock of the entries that passed.
    pub lock: Lock,
    /// Whether to write it: the policy keeps it and it differs from the Lock on disk.
    pub write_lock: bool,
    pub coverage: Coverage<'a>,
    /// The failures' diagnostics, in the order they happened.
    pub errors: Vec<miette::Report>,
    /// Whether the edit may be written to `refs.toml`.
    pub accepted: bool,
}

/// Decide, from what the executor found, what stage 1 leaves behind. `passed` are the
/// entries that resolved (or were reused) and verified; `failures` are the other active Repos,
/// in the order they failed.
pub fn settle<'a>(
    active: &ActiveSet<'a>,
    old: Option<&Lock>,
    passed: Vec<LockedRepo>,
    failures: Vec<Failure>,
    keep: Keep,
) -> Settled<'a> {
    let accepted = failures.is_empty() || keep == Keep::Passing;
    let lock = Lock::new(passed);
    let write_lock = accepted && old != Some(&lock);
    let (withheld, errors): (Vec<String>, Vec<miette::Report>) =
        failures.into_iter().map(|f| (f.id, f.error)).unzip();
    Settled {
        lock,
        write_lock,
        coverage: Coverage {
            active: active.without(&withheld),
            withheld,
        },
        errors,
        accepted,
    }
}
