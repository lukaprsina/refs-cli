//! A scripted `Updater` for tests of the CLI: no network, no receipt, no install.

use std::cell::RefCell;

use super::{Installed, SelfUpdateError, Updater};

/// One call the fake received, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    Check,
    Install,
}

pub struct FakeUpdater {
    outcome: Result<Option<Installed>, SelfUpdateError>,
    calls: RefCell<Vec<Call>>,
}

impl FakeUpdater {
    fn with(outcome: Result<Option<Installed>, SelfUpdateError>) -> Self {
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

    /// The lookup or the install failed with `why`.
    pub fn failing(why: &str) -> Self {
        Self::with(Err(SelfUpdateError::Failed(why.into())))
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
}

impl Updater for FakeUpdater {
    fn is_update_available(&self) -> Result<bool, SelfUpdateError> {
        self.calls.borrow_mut().push(Call::Check);
        self.outcome.clone().map(|installed| installed.is_some())
    }

    fn install(&self) -> Result<Option<Installed>, SelfUpdateError> {
        self.calls.borrow_mut().push(Call::Install);
        self.outcome.clone()
    }
}

/// An updater with no installer behind it: no receipt for this binary. It keeps no calls, so a
/// test can borrow it without a local.
pub struct Unmanaged;

impl Updater for Unmanaged {
    fn is_update_available(&self) -> Result<bool, SelfUpdateError> {
        Err(SelfUpdateError::NotInstalledByInstaller)
    }

    fn install(&self) -> Result<Option<Installed>, SelfUpdateError> {
        Err(SelfUpdateError::NotInstalledByInstaller)
    }
}
