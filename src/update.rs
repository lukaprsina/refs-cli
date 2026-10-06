//! `refs update`: replace the running binary with the latest release. The release lookup and
//! the install go through the `Updater` seam, so tests script the outcomes and only
//! `cli::run` puts the network behind it (`Axo`, axoupdater from cargo-dist). Nothing here
//! decides anything: it reports what the installer did.

use axoupdater::{AxoUpdater, AxoupdateError};
use miette::Diagnostic;
use thiserror::Error;

#[cfg(any(test, feature = "testing"))]
pub mod fake;

/// The package name cargo-dist keys the install receipt by: not the binary's name, `refs`.
const APP: &str = "refs-cli";

/// A release that `install` put in place of the running one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The version that was running, when the receipt says.
    pub from: Option<String>,
    pub to: String,
}

/// Why an update could not be checked or installed.
#[derive(Debug, Clone, PartialEq, Eq, Error, Diagnostic)]
pub enum UpdateError {
    /// No install receipt for this binary: it was not put here by the cargo-dist installer.
    #[error("this `refs` was not installed with the cargo-dist installer")]
    #[diagnostic(
        code(refs::update::not_installed_by_installer),
        help("update it the way you installed it")
    )]
    NotInstalledByInstaller,

    #[error("could not update: {0}")]
    #[diagnostic(code(refs::update::failed))]
    Failed(String),
}

/// Finds and installs releases of `refs`.
pub trait Updater {
    /// Whether a release newer than the running one exists. Installs nothing.
    fn is_update_available(&self) -> Result<bool, UpdateError>;

    /// Install the latest release over the running binary: `None` when it already is.
    fn install(&self) -> Result<Option<Installed>, UpdateError>;
}

/// The updater for a run with no installer behind it, such as `cli::run_with`: it never
/// reaches the network, and finds no receipt.
pub struct Unmanaged;

impl Updater for Unmanaged {
    fn is_update_available(&self) -> Result<bool, UpdateError> {
        Err(UpdateError::NotInstalledByInstaller)
    }

    fn install(&self) -> Result<Option<Installed>, UpdateError> {
        Err(UpdateError::NotInstalledByInstaller)
    }
}

/// The real updater: axoupdater, which reads the receipt the cargo-dist installer wrote and
/// asks GitHub Releases for the latest release.
pub struct Axo;

impl Axo {
    /// An updater loaded from this binary's receipt. A receipt for a binary in another
    /// directory counts as none. axoupdater compares only directories, so a `cargo install`
    /// into the installer's own directory (`CARGO_HOME/bin`) still passes.
    fn load() -> Result<AxoUpdater, UpdateError> {
        let mut updater = AxoUpdater::new_for(APP);
        // The installer would write to our stdout and stderr, past `-q`; a failed install
        // carries its captured output in the error instead.
        updater.disable_installer_output();
        updater.load_receipt().map_err(from_axo)?;
        match updater.check_receipt_is_for_this_executable() {
            Ok(true) => Ok(updater),
            Ok(false) => Err(UpdateError::NotInstalledByInstaller),
            Err(e) => Err(from_axo(e)),
        }
    }
}

impl Updater for Axo {
    fn is_update_available(&self) -> Result<bool, UpdateError> {
        Self::load()?.is_update_needed_sync().map_err(from_axo)
    }

    fn install(&self) -> Result<Option<Installed>, UpdateError> {
        let installed = Self::load()?.run_sync().map_err(from_axo)?;
        Ok(installed.map(|done| Installed {
            from: done.old_version.map(|v| v.to_string()),
            to: done.new_version.to_string(),
        }))
    }
}

fn from_axo(error: AxoupdateError) -> UpdateError {
    match error {
        AxoupdateError::NoReceipt { .. } | AxoupdateError::ReceiptLoadFailed { .. } => {
            UpdateError::NotInstalledByInstaller
        }
        other => UpdateError::Failed(other.to_string()),
    }
}
