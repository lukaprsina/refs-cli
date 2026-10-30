//! The only seam to the outside world: everything git, network and disk sits behind `Source`.

#[cfg(any(test, feature = "testing"))]
pub mod fake;
pub mod git;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{Config, Repo, RepoRef, is_full_sha};
use crate::diagnostic::SourceError;
use crate::lock::Field;
use crate::outline::Outline;

/// How a run gets its `Source`, once the project is loaded: it needs the project's root and
/// config (the real one wants its `references_dir`). Tests return the fake.
pub trait MakeSource: FnOnce(&Path, &Config) -> Result<Box<dyn Source>, SourceError> {}

impl<F> MakeSource for F where F: FnOnce(&Path, &Config) -> Result<Box<dyn Source>, SourceError> {}

/// A resolved identity of a Repo: a commit of a git remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    url: String,
    #[serde(rename = "ref")]
    git_ref: String,
    sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
}

impl Pin {
    pub fn new(url: &str, git_ref: &str, sha: &str, branch: Option<&str>) -> Pin {
        Pin {
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

pub trait Source {
    /// Ref to commit. Asks the remote; never creates a Checkout.
    fn resolve(&self, repo: RepoRef) -> Result<Pin, SourceError>;
    /// The files of the pinned commit at the Repo's `paths` and `start`, which `sync` checks
    /// them against (`outline::verify`). Fetches the commit and its trees on a Cache miss,
    /// unless `offline`.
    fn files(&self, repo: RepoRef, pin: &Pin, offline: bool) -> Result<Vec<String>, SourceError>;
    /// Fetch the pinned commit and create or move the Checkout to it. `offline` forbids network
    /// access. `force` is `plan`'s business (ADR 0006), so `Source` never sees it.
    fn materialise(&self, repo: RepoRef, pin: &Pin, offline: bool) -> Result<(), SourceError>;
    /// The names of the remote's tags, sorted. Asks the remote (one round trip); never touches
    /// the Cache or a Checkout. Takes a URL, as the Repo it is for is not in the config yet.
    fn tags(&self, url: &str) -> Result<Vec<String>, SourceError>;
    /// Resolve `git_ref` of the remote at `url` and make the commit's trees local in the Cache
    /// (no blobs), so that its paths can be listed. Takes a URL, as the Repo it is for is not
    /// in the config yet. A bad URL or ref fails here, before anything is edited.
    fn outline(&self, url: &str, git_ref: &str) -> Result<Outline, SourceError>;
    /// The text of the blobs at `paths` in the commit `pin`, fetched in one batch, and never
    /// lazily. A path that is not in the tree is an error.
    fn read(
        &self,
        url: &str,
        pin: &Pin,
        paths: &[String],
    ) -> Result<Vec<(String, String)>, SourceError>;
    fn remove(&self, id: &str) -> Result<(), SourceError>;
    fn inspect(&self, id: &str) -> Result<Observed, SourceError>;
    /// The names of the directories in the references directory, whatever made them.
    fn list(&self) -> Result<Vec<String>, SourceError>;
}
