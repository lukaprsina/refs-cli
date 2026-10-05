//! How a run ends (ADR 0006): the one rule that turns stage 1's failures and stage 2's
//! result into the outcome and the order of the diagnostics.

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

/// The outcome and diagnostics of a whole run.
#[derive(Debug)]
pub struct Concluded {
    pub outcome: Outcome,
    pub diagnostics: Vec<miette::Report>,
}

/// Combine stage 1's errors with what stage 2 came to. Any stage 1 error fails the run,
/// whatever stage 2 found; otherwise the outcome is stage 2's. The diagnostics are stage 1's
/// errors first, then stage 2's.
pub fn conclude(
    stage_one: Vec<miette::Report>,
    stage_two: Outcome,
    stage_two_diagnostics: Vec<miette::Report>,
) -> Concluded {
    let outcome = if stage_one.is_empty() {
        stage_two
    } else {
        Outcome::Failed
    };
    let mut diagnostics = stage_one;
    diagnostics.extend(stage_two_diagnostics);
    Concluded {
        outcome,
        diagnostics,
    }
}
