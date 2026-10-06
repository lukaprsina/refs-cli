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

/// What happened to `refs.toml` in `edit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The edit gave the text it already had; nothing was written.
    Unchanged,
    /// `refs.toml` was written.
    Written,
    /// The edit itself failed (it names no such Repo, or the text does not parse): nothing was
    /// written.
    Rejected,
    /// Stage 1 failed against the edited config: nothing was written, and `--no-sync` would
    /// write it.
    Blocked,
}

/// The follow-up the CLI gives a user whose project a run left incomplete or failed, named
/// for why it is given. Decided by `hint`; `cli` only gives each its words (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// `refs.toml` was written without a sync (`--no-sync`): the project is incomplete.
    RunSync,
    /// A `sync` failed or was refused, or an edit that changed nothing did.
    RunSyncOnceFixed,
    /// `refs.toml` was written, then the sync failed or was refused.
    FixOrRemoveThenSync,
    /// The edit was rejected, so `refs.toml` is as it was.
    ConfigUnchanged,
    /// Stage 1 blocked the edit, so `refs.toml` is as it was; `--no-sync` writes it anyway.
    ConfigUnchangedTryNoSync,
}

/// The command a run was, as far as its hint goes. `lock` has no hint.
#[derive(Debug, Clone, Copy)]
pub enum Command {
    Sync,
    Edit { change: Change, no_sync: bool },
}

/// The hint of a run: at most one, since with `no_sync` an edit does not sync, so it cannot
/// also fail.
pub fn hint(command: Command, outcome: Outcome) -> Option<Hint> {
    let incomplete = matches!(outcome, Outcome::Failed | Outcome::Refused);
    match command {
        Command::Sync => incomplete.then_some(Hint::RunSyncOnceFixed),
        Command::Edit { change, no_sync } => {
            if no_sync && change == Change::Written {
                return Some(Hint::RunSync);
            }
            incomplete.then_some(match change {
                Change::Written => Hint::FixOrRemoveThenSync,
                Change::Rejected => Hint::ConfigUnchanged,
                Change::Blocked => Hint::ConfigUnchangedTryNoSync,
                Change::Unchanged => Hint::RunSyncOnceFixed,
            })
        }
    }
}
