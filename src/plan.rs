//! The sync decisions, as pure functions (ADR 0006): `lock` is stage 1, `checkouts` stage 2.

mod checkouts;
mod lock;

pub use checkouts::{
    Action, AgentFileText, Checkouts, Exclude, Plan, ProjectObserved, plan_checkouts,
};
pub use lock::{Drift, LockFlags, LockPlan, Step, Upgrade, lock_drift, plan_lock};
