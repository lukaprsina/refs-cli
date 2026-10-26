//! The Manifests of a looked-at repo that a Checkout will hold, read once what
//! `add` and `update` ask about `packages` and the description. `packages` chooses and reads the
//! text (pure); this fetches and counts what was left unread.

use crate::diagnostic::SourceError;
use crate::packages::{self, infer, manifest_paths};
use crate::registry::pin::Say;
use crate::source::{Outline, Source};

/// The Manifests (path and text) that were read. Empty when none was, which is also what a repo
/// with none gives.
#[derive(Debug, Default)]
pub struct Manifests(Vec<(String, String)>);

impl Manifests {
    /// The Manifests of the `outline`d repo that a Checkout of `paths` will hold, up to the cap
    /// (said when some were left unread). The sync may resolve another commit if upstream moved
    /// in between: accepted.
    pub fn read(
        source: &dyn Source,
        url: &str,
        outline: &Outline,
        paths: &[String],
        say: &mut dyn Say,
    ) -> Result<Self, SourceError> {
        let (chosen, left) = manifest_paths(&outline.paths, paths);
        if left > 0 {
            let read = chosen.len();
            say.status(format!("read {read} Manifests and left {left} unread"));
        }
        source.read(url, &outline.pin, &chosen).map(Self)
    }

    /// The names of the packages that a Checkout of `paths` gives.
    pub fn packages_for(&self, paths: &[String]) -> Vec<String> {
        packages::packages_for(&self.0, paths)
    }

    /// The description of the root Manifest, if it has one.
    pub fn description(&self) -> Option<String> {
        infer(&self.0).description
    }
}
