//! The only seam to the outside world: everything git, network and disk sits behind `Source`.

#[cfg(any(test, feature = "testing"))]
pub mod fake;

use serde::{Deserialize, Serialize};

use crate::config::{Repo, RepoRef, is_full_sha};
use crate::diagnostic::SourceError;
use crate::lock::Field;

/// A resolved identity of a Repo. Only this module interprets it; everything else
/// goes through the display surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pin(PinKind);

/// Tagged by `source`, which is how it appears in the Lock.
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
            PinKind::Git { sha, .. } => is_full_sha(sha),
        }
    }

    /// The fields of `repo` that no longer match what this pin resolved (`Paths` is the
    /// locked Repo's, not the pin's).
    pub fn drift_from(&self, repo: &Repo) -> Vec<Field> {
        match &self.0 {
            PinKind::Git { url, git_ref, .. } => [
                (url.as_str() != repo.url.as_ref(), Field::Url),
                (git_ref != repo.effective_ref(), Field::Ref),
            ]
            .into_iter()
            .filter_map(|(differs, field)| differs.then_some(field))
            .collect(),
        }
    }

    /// Whether both pins name the same commit of the same remote. The display-only
    /// `branch` does not count.
    pub fn same_commit(&self, other: &Pin) -> bool {
        match (&self.0, &other.0) {
            (
                PinKind::Git {
                    url, git_ref, sha, ..
                },
                PinKind::Git {
                    url: u,
                    git_ref: r,
                    sha: s,
                    ..
                },
            ) => url == u && git_ref == r && sha == s,
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
            } if git_ref == "HEAD" => branch.as_deref().unwrap_or(git_ref),
            PinKind::Git { git_ref, .. } => git_ref,
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
