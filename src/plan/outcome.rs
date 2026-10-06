//! How a run ends (ADR 0006): the outcome of `--check`, what became of an edit of
//! `refs.toml`, and the follow-up hint.

use super::checkouts::Plan;

/// How a run ended; the CLI maps it to an exit code (0, 3, 1, 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    InSync,
    OutOfDate,
    Refused,
    Failed,
}

/// The outcome of `sync --check`. `plan` is `None` when the Lock does not cover the active
/// Repos, so no Plan could be made: the Lock is what is out of date. `lock_drifted` is
/// whether the Lock differed from the config. A refusal outranks drift, since "run `refs
/// sync`" is the wrong advice when `sync` will also refuse.
pub fn check_outcome(plan: Option<&Plan>, lock_drifted: bool) -> Outcome {
    match plan {
        None => Outcome::OutOfDate,
        Some(plan) if !plan.refusals.is_empty() => Outcome::Refused,
        Some(plan) if plan.is_drift() || lock_drifted => Outcome::OutOfDate,
        Some(_) => Outcome::InSync,
    }
}

/// The follow-up the CLI gives a user whose project a run left incomplete or failed, named
/// for why it is given. `sync` sets it where it knows why; `cli` only gives each its words
/// (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// `refs.toml` was written without a sync (`--no-sync`): the project is incomplete.
    RunSync,
    /// A `sync` failed or was refused, or an edit that changed nothing did.
    RunSyncOnceFixed,
    /// `refs.toml` was written, then the sync failed or was refused.
    FixOrRemoveThenSync,
    /// The edit was rejected, or a write failed, so `refs.toml` is as it was and `--no-sync`
    /// would not help.
    ConfigUnchanged,
    /// Stage 1 blocked the edit, so `refs.toml` is as it was; `--no-sync` writes it anyway.
    ConfigUnchangedTryNoSync,
}
