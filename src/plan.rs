//! The sync decisions, as pure functions (ADR 0006): `lock` is stage 1, `checkouts` stage 2.

mod checkouts;
mod classify;
mod lock;
mod outcome;

pub use checkouts::{
    AgentFileText, Checkout, Checkouts, Exclude, ExcludeAction, Placement, Plan, ProjectObserved,
    RepoAction, WriteAgentFile, plan_checkouts,
};
pub use classify::{Cause, CheckoutState, classify};
pub use lock::{
    Drift, LockFlags, LockPlan, Step, Upgrade, lock_drift, locked, plan_lock, shrunk_lock,
};
pub use outcome::{Hint, Outcome, check_outcome};
