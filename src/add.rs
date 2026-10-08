//! `refs add` after the prompts (ADR 0009): add the repo and sync, then read the new
//! Checkout's Manifests for its `packages`, confirm them, and write them with a second sync
//! that only re-renders the Managed block.

use std::path::Path;

use crate::config::Config;
use crate::edit::{AddRepo, Edit};
use crate::packages::infer_packages;
use crate::prompt::{self, Prompter};
use crate::source::Source;
use crate::sync::{self, Outcome, Report, SyncFlags};

/// What an `add` did, for the CLI to print.
#[derive(Debug)]
pub struct Added {
    /// The add and its sync, then the sync that wrote `packages` if there was one.
    pub reports: Vec<Report>,
    /// The person gave up the `packages` question, so the repo has none.
    pub packages_skipped: bool,
}

/// Add `repo` to the project at `root` and sync it. Unless `repo` names its `packages`, they
/// are then read from the Checkout: asked for with the names found as the default when there
/// is a `prompter`, otherwise the names found are written.
pub fn run(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    repo: &AddRepo,
    prompter: Option<&mut dyn Prompter>,
) -> Added {
    let first = sync::edit(source, root, &Edit::Add(repo), &SyncFlags::default());
    let mut added = Added {
        reports: Vec::new(),
        packages_skipped: false,
    };
    let infer = repo.packages.is_empty() && first.outcome == Outcome::InSync;
    added.reports.push(first);
    if !infer {
        return added;
    }
    let id = repo.resolved_id();
    let checkout = root.join(config.settings.references_dir()).join(&id);
    let inferred = infer_packages(&checkout);
    let names = match prompter {
        Some(prompter) => match prompt::ask_packages(&inferred, prompter) {
            Ok(names) => names,
            Err(_) => {
                added.packages_skipped = true;
                return added;
            }
        },
        None => inferred,
    };
    if !names.is_empty() {
        let flags = SyncFlags {
            offline: true,
            ..SyncFlags::default()
        };
        let edit = Edit::SetPackages {
            id: &id,
            names: &names,
        };
        added.reports.push(sync::edit(source, root, &edit, &flags));
    }
    added
}
