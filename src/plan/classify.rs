//! What is on disk for one Repo, against what the Lock says it should be.

use crate::config::Repo;
use crate::source::{Observed, Pin};

/// Where one active, locked Repo's Checkout stands (GLOSSARY: **Checkout state**).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutState {
    /// A Checkout of the locked commit with the Paths the config asks for.
    InSync,
    Absent,
    /// A Checkout whose Cache history is gone.
    Dangling,
    /// A directory that is not one of ours is in the way.
    Foreign,
    /// One of ours, but not what the Lock says. `dirty_files` is empty when it is clean.
    Stale {
        cause: Cause,
        dirty_files: Vec<String>,
    },
}

/// Why a Checkout is `Stale`. A different commit wins over different Paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Commit,
    Paths,
}

/// Classify `observed` against the Repo's config and the Pin the Lock holds for it. The one
/// rule behind the checkout stage of the Plan: a Checkout is in sync when it is of the locked
/// commit with the Paths `repo` asks for, as a set (a `Pin`'s `branch` does not count).
pub fn classify(observed: &Observed, repo: &Repo, locked: &Pin) -> CheckoutState {
    match observed {
        Observed::Absent => CheckoutState::Absent,
        Observed::Dangling => CheckoutState::Dangling,
        Observed::Foreign => CheckoutState::Foreign,
        Observed::At {
            pin, dirty_files, ..
        } => {
            let cause = if observed.matches(repo, locked) {
                return CheckoutState::InSync;
            } else if !pin.same_commit(locked) {
                Cause::Commit
            } else {
                Cause::Paths
            };
            CheckoutState::Stale {
                cause,
                dirty_files: dirty_files.clone(),
            }
        }
    }
}
