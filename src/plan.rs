//! The sync decisions, as pure functions (ADR 0006): `lock` is stage 1, `checkouts` stage 2.

mod checkouts;
mod lock;

pub use checkouts::{
    AgentFileText, Checkouts, Exclude, ExcludeAction, Outcome, Plan, ProjectObserved, RepoAction,
    WriteAgentFile, check_outcome, plan_checkouts,
};
pub use lock::{Drift, LockFlags, LockPlan, NewLock, Step, Upgrade, lock_drift, locked, plan_lock};
