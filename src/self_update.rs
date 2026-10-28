//! `refs self update`: replace the running binary with the latest release. The release lookup and
//! the install go through the `Updater` seam, so tests script the outcomes and only
//! `cli::run` puts the network behind it (`Axo`, axoupdater from cargo-dist). Nothing here
//! decides anything: it reports what the installer did.

use axoupdater::{AxoUpdater, AxoupdateError};
use miette::Diagnostic;
use thiserror::Error;

use crate::ui::{Console, EXIT_OUT_OF_DATE, fail};

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
pub enum SelfUpdateError {
    /// No install receipt for this binary: it was not put here by the cargo-dist installer.
    #[error("this `refs` was not installed with the cargo-dist installer")]
    #[diagnostic(
        code(refs::self_update::not_installed_by_installer),
        help("update it the way you installed it")
    )]
    NotInstalledByInstaller,

    #[error("could not update: {0}")]
    #[diagnostic(code(refs::self_update::failed))]
    Failed(String),
}

/// Finds and installs releases of `refs`.
pub trait Updater {
    /// Whether a release newer than the running one exists. Installs nothing.
    fn is_update_available(&self) -> Result<bool, SelfUpdateError>;

    /// Install the latest release over the running binary: `None` when it already is.
    fn install(&self) -> Result<Option<Installed>, SelfUpdateError>;
}

/// `refs self update`: install the latest release over this one, or with `--check` only say
/// whether there is one (exit 3, as `sync --check` does for a project that is out of date).
pub(crate) fn run(updater: &dyn Updater, check: bool, console: &mut Console) -> u8 {
    if check {
        return match updater.is_update_available() {
            Ok(true) => {
                console
                    .problem("out of date: a newer release is available; run `refs self update`");
                EXIT_OUT_OF_DATE
            }
            Ok(false) => {
                console.status("up to date");
                0
            }
            Err(e) => fail(e, console),
        };
    }
    match updater.install() {
        Ok(Some(installed)) => {
            let from = installed
                .from
                .map_or(String::new(), |v| format!(" from {v}"));
            console.status(format_args!("updated refs{from} to {}", installed.to));
            0
        }
        Ok(None) => {
            console.status("up to date");
            0
        }
        Err(e) => fail(e, console),
    }
}

/// The real updater: axoupdater, which reads the receipt the cargo-dist installer wrote and
/// asks GitHub Releases for the latest release.
pub struct Axo;

impl Axo {
    /// An updater loaded from this binary's receipt. A receipt for a binary in another
    /// directory counts as none. axoupdater compares only directories, so a `cargo install`
    /// into the installer's own directory (`CARGO_HOME/bin`) still passes.
    fn load() -> Result<AxoUpdater, SelfUpdateError> {
        let mut updater = AxoUpdater::new_for(APP);
        // The installer would write to our stdout and stderr, past `-q`; a failed install
        // carries its captured output in the error instead.
        updater.disable_installer_output();
        // axoupdater does not read this itself. Unauthenticated release lookups hit GitHub's
        // rate limit fast from CI; locally, `AXOUPDATER_GITHUB_TOKEN=$(gh auth token)` does it.
        if let Some(token) = std::env::var("AXOUPDATER_GITHUB_TOKEN")
            .ok()
            .filter(|token| !token.is_empty())
        {
            updater.set_github_token(&token);
        }
        updater.load_receipt().map_err(from_axo)?;
        match updater.check_receipt_is_for_this_executable() {
            Ok(true) => Ok(updater),
            Ok(false) => Err(SelfUpdateError::NotInstalledByInstaller),
            Err(e) => Err(from_axo(e)),
        }
    }
}

impl Updater for Axo {
    fn is_update_available(&self) -> Result<bool, SelfUpdateError> {
        Self::load()?.is_update_needed_sync().map_err(from_axo)
    }

    fn install(&self) -> Result<Option<Installed>, SelfUpdateError> {
        let installed = Self::load()?.run_sync().map_err(from_axo)?;
        Ok(installed.map(|done| Installed {
            from: done.old_version.map(|v| v.to_string()),
            to: done.new_version.to_string(),
        }))
    }
}

fn from_axo(error: AxoupdateError) -> SelfUpdateError {
    match error {
        AxoupdateError::NoReceipt { .. } | AxoupdateError::ReceiptLoadFailed { .. } => {
            SelfUpdateError::NotInstalledByInstaller
        }
        other => SelfUpdateError::Failed(other.to_string()),
    }
}
