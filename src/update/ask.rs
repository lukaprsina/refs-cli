//! The questions of `update` (ADR 0012): what to re-ask of a Repo that its new Ref breaks.

use crate::config::Repo;
use crate::edit::Fields;
use crate::listing::{self, Gone};
use crate::manifests::Manifests;
use crate::ui::Say;
use crate::ui::pickers::{ask_packages, ask_paths, ask_start};
use crate::ui::{Abort, Prompter};

/// Ask `Update?` once `mend` has said what the new Ref broke. On yes, re-ask only the broken fields of `repo`, each starting from `refs.toml`
/// with the entries that are still valid ticked and the missing ones offered unticked; the
/// `packages` are kept, and asked again only when the `paths` were. `listed` is the files of the
/// new tree and `manifests` its Manifests. On no there is nothing to change.
pub(super) fn fill_update(
    repo: &Repo,
    listed: &[String],
    gone: &Gone,
    manifests: &Manifests,
    prompter: &mut dyn Prompter,
    say: &mut dyn Say,
) -> Result<Option<Fields>, Abort> {
    if !prompter.confirm("Update?")? {
        return Ok(None);
    }
    let valid = |items: Vec<String>, gone: &[String]| -> Vec<String> {
        items.into_iter().filter(|i| !gone.contains(i)).collect()
    };
    let listed = Some(listed);
    let mut fixed = Fields::default();
    let mut paths = repo.path_strings();
    if !gone.paths.is_empty() {
        let ticked = valid(paths.clone(), &gone.paths);
        paths = ask_paths(prompter, listed, (&paths, &ticked), say)?;
        let kept = repo.package_strings();
        let offered = listing::with_ticked(manifests.packages_for(&paths), &kept);
        if !offered.is_empty() {
            fixed.packages = Some(ask_packages(prompter, &offered, &kept)?);
        }
        fixed.paths = Some(paths.clone());
    }
    if !gone.start.is_empty() {
        let offered = repo.start_strings();
        let ticked = valid(offered.clone(), &gone.start);
        fixed.start = Some(ask_start(
            prompter,
            listed,
            &paths,
            (&offered, &ticked),
            say,
        )?);
    }
    Ok(Some(fixed))
}
