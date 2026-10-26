//! `refs add` (ADR 0008, 0009, 0011), all but the printing of its report (it says what it read and
//! decided, and warns, through a `Say`). First the repo it is asked to add is made ready: a
//! registry or forge shorthand expanded, its version pinned to a tag (`registry::pin`), the ref
//! asked for, and the repo looked at (`Source::outline`), so that a bad URL or ref fails before
//! the other questions. Its Manifests are read then, for the `packages` and the description.
//! Then the repo is added and synced once. With no sync (`--no-sync`) nothing is looked at, so
//! the `packages` stay as given.

use std::path::Path;

mod ask;

use crate::config::Config;
use crate::edit::{AddRepo, Edit};
use crate::failure::Failure;
use crate::manifests::Manifests;
use crate::prompt::Prompter;
use crate::registry::pin::{Pinned, Say, pin};
use crate::registry::{self, Registry};
use crate::source::Source;
use crate::sync::{self, Report, SyncFlags};

/// Who `add` can ask and what it can ask about.
pub struct Asking<'a> {
    /// The person to ask, when stdin and stderr are terminals; `add` drops it under `--no-input`.
    pub prompter: Option<&'a mut dyn Prompter>,
    pub registry: &'a dyn Registry,
}

impl Asking<'_> {
    pub fn prompter(&mut self) -> Option<&mut dyn Prompter> {
        self.prompter
            .as_mut()
            .map(|p| &mut **p as &mut dyn Prompter)
    }
}

/// Add `repo`, as the command line gave it, to the project at `root` and sync it: one edit of
/// `refs.toml` and one sync. It is first made ready (see `prepare`), and when there is someone
/// to ask, the equivalent command line is said before the edit, so a failed sync does not lose
/// it. `flags` are for the sync. Without a `source` (`--no-sync`) the repo is only added: there
/// are no tags to list and no repo to look at, so the version is not pinned and the `packages`
/// stay as given.
pub fn run(
    source: Option<&dyn Source>,
    root: &Path,
    config: &Config,
    repo: &AddRepo,
    flags: &SyncFlags,
    asking: &mut Asking,
    say: &mut dyn Say,
) -> Result<Report, Failure> {
    let repo = prepare(repo, source, root, config, flags.offline, asking, say)?;
    if asking.prompter.is_some() {
        say.status(format!("equivalent: {}", ask::equivalent_command(&repo)));
    }
    let edit = Edit::Add(&repo);
    Ok(match source {
        Some(source) => sync::edit(source, root, &edit, flags),
        None => sync::edit_only(root, &edit),
    })
}

/// `repo` ready to add: a registry shorthand expanded and its version pinned to a tag, the repo
/// looked at for its `packages` and description, then what the command line left out asked
/// for. The version is the one given, else the one the project's Package lockfile at `root` says
/// it uses. The tags come from `source`. Only a missing URL is a problem without a prompter. On a
/// cancel nothing has been edited.
fn prepare(
    repo: &AddRepo,
    source: Option<&dyn Source>,
    root: &Path,
    config: &Config,
    offline: bool,
    asking: &mut Asking,
    say: &mut dyn Say,
) -> Result<AddRepo, Failure> {
    let mut repo = repo.clone();
    if repo.url.is_empty()
        && let Some(prompter) = asking.prompter()
    {
        repo.url = ask::ask_url(prompter).map_err(Failure::Asked)?;
    }
    let paths_given = !repo.paths.is_empty();
    let expanded = registry::expand(&repo, asking.registry, offline).map_err(Failure::Registry)?;
    if let Some(ignored) = &expanded.ignored {
        let shorthand = &repo.url;
        say.warning(format!(
            "ignored `{ignored}` after the repository in `{shorthand}`; \
             use `--ref` and `--paths` to choose a ref and directories"
        ));
    }
    // A registry add has chosen its Ref (a tag, or the default branch) unless nothing was pinned.
    let registry_add = expanded.requested.is_some();
    let mut ask_ref = !registry_add;
    let registry_description = expanded.description;
    let mut repo = expanded.repo;
    // A `ref` given with a version was refused by `expand`; without one it is kept as it is.
    if let (None, Some(requested)) = (&repo.git_ref, &expanded.requested) {
        let version = requested.version.as_deref();
        let package = &requested.package;
        let url = &repo.url;
        let prompter = asking.prompter();
        repo.git_ref = match pin(package, version, url, root, source, prompter, say)? {
            Pinned::Tag(tag) => Some(tag),
            Pinned::FollowsDefault => None,
            Pinned::Unpinned => {
                ask_ref = true;
                None
            }
        };
    }
    if repo.git_ref.is_none()
        && ask_ref
        && let Some(prompter) = asking.prompter()
    {
        repo.git_ref = ask::ask_ref(prompter).map_err(Failure::Asked)?;
    }
    // Looking at the repo comes before the rest of the questions, so that a bad URL or ref
    // fails before they are asked and with nothing edited.
    let looks = source.filter(|_| {
        !repo.url.is_empty() && !offline && (asking.prompter.is_some() || repo.packages.is_empty())
    });
    let mut described = registry_description;
    let mut manifests = Manifests::default();
    let mut outline = None;
    if let Some(source) = looks {
        let git_ref = repo.git_ref.as_deref().unwrap_or("HEAD");
        let looked = source
            .outline(&repo.url, git_ref)
            .map_err(Failure::Source)?;
        // A registry add that names its package and describes it has no use for the Manifests.
        let nameless = repo.packages.is_empty();
        let undescribed = described.is_none() && repo.description.is_none();
        if nameless || undescribed {
            manifests = Manifests::read(source, &repo.url, &looked, &repo.paths, say)
                .map_err(Failure::Source)?;
            described = described.or(manifests.description());
        }
        outline = Some(looked);
    }
    // Only the Manifests a Checkout of `paths` will hold count. None is fetched again.
    let Some(prompter) = asking.prompter() else {
        if repo.packages.is_empty() {
            repo.packages = manifests.packages_for(&repo.paths);
        }
        return match repo.url.is_empty() {
            true => Err(Failure::Usage(
                "a URL is required when not run in a terminal".into(),
            )),
            false => Ok(repo),
        };
    };
    let known = ask::Known {
        described: described.as_deref(),
        looked_at: outline.as_ref(),
        // The directory the registry gave is where the picker starts, unlike one from `--paths`.
        paths_from_registry: registry_add && !paths_given && outline.is_some(),
        manifests: outline.is_some().then_some(&manifests),
    };
    ask::fill_add(&mut repo, config, prompter, &known, say).map_err(Failure::Asked)?;
    Ok(repo)
}
