//! The sync decisions, as pure functions (ADR 0006): `lock` is stage 1, `checkouts` stage 2 (`sync` runs both).

mod checkouts;
mod classify;
mod lock;

pub use checkouts::{
    AgentFileText, Checkout, Checkouts, Exclude, Outcome, Placement, Plan, ProjectObserved,
    RepoAction, WriteAgentFile, plan_checkouts,
};
pub use classify::{Cause, CheckoutState, classify};
pub use lock::{Drift, Step, lock_drift, plan_lock, shrunk_lock};
