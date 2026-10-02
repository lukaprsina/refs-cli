//! The only seam to the outside world: everything git, network and disk sits behind `Source`.

#[cfg(any(test, feature = "testing"))]
pub mod fake;

use serde::{Deserialize, Serialize};

use crate::config::RepoRef;
use crate::diagnostic::SourceError;

/// A resolved identity of a Repo. Only this module interprets it; everything else
/// goes through the display surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pin(PinKind);

/// Tagged by `source`, which is how it appears in a lock entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
enum PinKind {
    Git {
        url: String,
        #[serde(rename = "ref")]
        git_ref: String,
        sha: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
}

impl Pin {
    pub fn git(url: &str, git_ref: &str, sha: &str, branch: Option<&str>) -> Pin {
        Pin(PinKind::Git {
            url: url.into(),
            git_ref: git_ref.into(),
            sha: sha.into(),
            branch: branch.map(Into::into),
        })
    }

    /// Whether the pin is well formed (a full 40-hex commit id), for input read from disk.
    pub fn is_well_formed(&self) -> bool {
        match &self.0 {
            PinKind::Git { sha, .. } => {
                sha.len() == 40 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            }
        }
    }

    /// The first 7 characters of the commit id, for the block.
    pub fn short_id(&self) -> &str {
        match &self.0 {
            PinKind::Git { sha, .. } => sha.get(..7).unwrap_or(sha),
        }
    }

    /// What the block shows after `@`: the remote's branch for a `HEAD` ref, else the ref.
    pub fn display_ref(&self) -> &str {
        match &self.0 {
            PinKind::Git {
                git_ref, branch, ..
            } => branch.as_deref().unwrap_or(git_ref),
        }
    }
}

/// What is on disk for one Repo. `Source` reports it; `plan` decides what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    Absent,
    Dangling,
    Foreign,
    At {
        pin: Pin,
        paths: Vec<String>,
        dirty_files: Vec<String>,
    },
}

/// Options for `materialise`: `offline` forbids network access. `force` is `plan`'s
/// business (ADR 0004), so `Source` never sees it.
#[derive(Debug, Clone, Copy, Default)]
pub struct MaterialiseOpts {
    pub offline: bool,
}

pub trait Source {
    /// Ref to commit. Asks the remote; never creates a Checkout.
    fn resolve(&self, repo: RepoRef) -> Result<Pin, SourceError>;
    /// Check that `paths` and `start` exist at the pinned commit, using cached objects only.
    fn verify(&self, repo: RepoRef, pin: &Pin) -> Result<(), SourceError>;
    /// Fetch the pinned commit and create or move the Checkout to it.
    fn materialise(
        &self,
        repo: RepoRef,
        pin: &Pin,
        opts: MaterialiseOpts,
    ) -> Result<(), SourceError>;
    fn remove(&self, id: &str) -> Result<(), SourceError>;
    fn inspect(&self, id: &str) -> Result<Observed, SourceError>;
}
