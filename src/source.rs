//! The only seam to the outside world: everything git, network and disk sits behind `Source`.

#[cfg(any(test, feature = "testing"))]
pub mod fake;
pub mod git;

use serde::{Deserialize, Serialize};

use crate::config::{Repo, RepoRef, is_full_sha};
use crate::diagnostic::SourceError;
use crate::lock::Field;

/// A resolved identity of a Repo: a commit of a remote. In the Lock the `source` tag says
/// where it came from; `git` is the only one, and any other is rejected on read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    source: PinSource,
    url: String,
    #[serde(rename = "ref")]
    git_ref: String,
    sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum PinSource {
    Git,
}

impl Pin {
    pub fn git(url: &str, git_ref: &str, sha: &str, branch: Option<&str>) -> Pin {
        Pin {
            source: PinSource::Git,
            url: url.into(),
            git_ref: git_ref.into(),
            sha: sha.into(),
            branch: branch.map(Into::into),
        }
    }

    /// The remote.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The full commit id.
    pub fn sha(&self) -> &str {
        &self.sha
    }

    /// Whether the pin is well formed (a full 40-hex commit id), for input read from disk.
    pub fn is_well_formed(&self) -> bool {
        is_full_sha(&self.sha)
    }

    /// The fields of `repo` that no longer match what this pin resolved. Empty means the
    /// pin still serves the Repo: this is the one rule behind lock drift and pin reuse.
    pub fn drift_from(&self, repo: &Repo) -> Vec<Field> {
        [
            (self.url.as_str() != repo.url.as_ref(), Field::Url),
            (self.git_ref != repo.effective_ref(), Field::Ref),
        ]
        .into_iter()
        .filter_map(|(differs, field)| differs.then_some(field))
        .collect()
    }

    /// Whether both pins name the same commit of the same remote. The display-only
    /// `branch` does not count.
    pub fn same_commit(&self, other: &Pin) -> bool {
        self.url == other.url && self.git_ref == other.git_ref && self.sha == other.sha
    }

    /// The first 7 characters of the commit id, for the block.
    pub fn short_id(&self) -> &str {
        self.sha.get(..7).unwrap_or(&self.sha)
    }

    /// What the block shows after `@`: the remote's branch for a `HEAD` ref, else the ref.
    pub fn display_ref(&self) -> &str {
        match &self.branch {
            Some(branch) if self.git_ref == "HEAD" => branch,
            _ => &self.git_ref,
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
/// business (ADR 0006), so `Source` never sees it.
#[derive(Debug, Clone, Copy, Default)]
pub struct MaterialiseOpts {
    pub offline: bool,
}

/// Options for `verify`: `offline` forbids network access, so a Commit missing from the
/// Cache is an error instead of a fetch.
#[derive(Debug, Clone, Copy, Default)]
pub struct VerifyOpts {
    pub offline: bool,
}

pub trait Source {
    /// Ref to commit. Asks the remote; never creates a Checkout.
    fn resolve(&self, repo: RepoRef) -> Result<Pin, SourceError>;
    /// Check that `paths` and `start` exist at the pinned commit. Fetches the commit and its
    /// trees on a Cache miss, unless `opts.offline`.
    fn verify(&self, repo: RepoRef, pin: &Pin, opts: VerifyOpts) -> Result<(), SourceError>;
    /// Fetch the pinned commit and create or move the Checkout to it.
    fn materialise(
        &self,
        repo: RepoRef,
        pin: &Pin,
        opts: MaterialiseOpts,
    ) -> Result<(), SourceError>;
    fn remove(&self, id: &str) -> Result<(), SourceError>;
    fn inspect(&self, id: &str) -> Result<Observed, SourceError>;
    /// The names of the directories in the references directory, whatever made them.
    fn list(&self) -> Result<Vec<String>, SourceError>;
}
