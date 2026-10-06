//! A scripted `Updater` for tests of the CLI: no network, no receipt, no install.

use std::cell::RefCell;

use super::{Installed, UpdateError, Updater};

/// One call the fake received, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    Check,
    Install,
}

pub struct FakeUpdater {
    outcome: Result<Option<Installed>, UpdateError>,
    calls: RefCell<Vec<Call>>,
}

impl FakeUpdater {
    fn with(outcome: Result<Option<Installed>, UpdateError>) -> Self {
        Self {
            outcome,
            calls: RefCell::default(),
        }
    }

    /// Running `from`, with `to` the latest release.
    pub fn release(from: &str, to: &str) -> Self {
        Self::with(Ok(Some(Installed {
            from: Some(from.into()),
            to: to.into(),
        })))
    }

    /// Already running the latest release.
    pub fn current() -> Self {
        Self::with(Ok(None))
    }

    /// No install receipt for this binary.
    pub fn unmanaged() -> Self {
        Self::with(Err(UpdateError::NotInstalledByInstaller))
    }

    /// The lookup or the install failed with `why`.
    pub fn failing(why: &str) -> Self {
        Self::with(Err(UpdateError::Failed(why.into())))
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
}

impl Updater for FakeUpdater {
    fn is_update_available(&self) -> Result<bool, UpdateError> {
        self.calls.borrow_mut().push(Call::Check);
        self.outcome.clone().map(|installed| installed.is_some())
    }

    fn install(&self) -> Result<Option<Installed>, UpdateError> {
        self.calls.borrow_mut().push(Call::Install);
        self.outcome.clone()
    }
}
