//! `refs add` (ADR 0008, 0009, 0011), all but the printing of its report (it says what it read and
//! decided, and warns, through a `Say`). First the repo it is asked to add is made ready: a
//! registry or forge shorthand expanded, its version pinned to a tag (`pin`), the ref
//! asked for, and the repo looked at (`Source::outline`), so that a bad URL or ref fails before
//! the other questions. Its Manifests are read then, for the `packages` and the description.
//! Then the repo is added and synced once. With no sync (`--no-sync`) nothing is looked at, so
//! the `packages` stay as given.

use std::path::Path;

mod ask;
mod expand;
pub mod pin;

use crate::config::Config;
use crate::edit::{AddRepo, Edit};
use crate::failure::Failure;
use crate::manifests::Manifests;
use crate::registry::Registry;
use crate::source::{Outline, Source};
use crate::sync::{self, Report, SyncFlags};
use crate::ui::{Prompter, Say};
use expand::{Expanded, Requested, expand};
use pin::{Pinned, pin};

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
    let mut cx = Context {
        source,
        root,
        offline: flags.offline,
        asking,
        say,
    };
    let repo = prepare(repo, config, &mut cx)?;
    if cx.asking.prompter.is_some() {
        cx.say
            .status(format!("equivalent: {}", ask::equivalent_command(&repo)));
    }
    let edit = Edit::Add(&repo);
    Ok(match source {
        Some(source) => sync::edit(source, root, &edit, flags),
        None => sync::edit_only(root, &edit),
    })
}

/// What the steps of `add` work with besides the repo: where it can look, who it can ask and
/// how it says what it did.
struct Context<'a, 'b> {
    source: Option<&'a dyn Source>,
    root: &'a Path,
    offline: bool,
    asking: &'a mut Asking<'b>,
    say: &'a mut dyn Say,
}

/// `repo` ready to add: a registry shorthand expanded and its version pinned to a tag, the repo
/// looked at for its `packages` and description, then what the command line left out asked
/// for. The version is the one given, else the one the project's Package lockfile at `root` says
/// it uses. The tags come from `source`. Only a missing URL is a problem without a prompter. On a
/// cancel nothing has been edited.
fn prepare(repo: &AddRepo, config: &Config, cx: &mut Context) -> Result<AddRepo, Failure> {
    let mut repo = repo.clone();
    if repo.url.is_empty()
        && let Some(prompter) = cx.asking.prompter()
    {
        repo.url = ask::ask_url(prompter).map_err(Failure::Asked)?;
    }
    let paths_given = !repo.paths.is_empty();
    let Expanded {
        mut repo,
        requested,
        description,
        ..
    } = expand(&repo, cx.asking.registry, cx.offline, &mut *cx.say).map_err(Failure::Registry)?;
    // A registry add has chosen its Ref (a tag, or the default branch) unless nothing was pinned.
    let registry_add = requested.is_some();
    cx.settle_ref(&mut repo, requested.as_ref())?;
    let looked = cx.look(&repo, description)?;
    // Only the Manifests a Checkout of `paths` will hold count. None is fetched again.
    let Some(prompter) = cx.asking.prompter() else {
        if repo.packages.is_empty() {
            repo.packages = looked.manifests.packages_for(&repo.paths);
        }
        return match repo.url.is_empty() {
            true => Err(Failure::Usage(
                "a URL is required when not run in a terminal".into(),
            )),
            false => Ok(repo),
        };
    };
    // The directory the registry gave is where the picker starts, unlike one from `--paths`.
    let paths_from_registry = registry_add && !paths_given && looked.outline.is_some();
    ask::fill_add(
        &mut repo,
        config,
        prompter,
        &looked,
        paths_from_registry,
        &mut *cx.say,
    )
    .map_err(Failure::Asked)?;
    Ok(repo)
}

/// What looking at the repo found.
struct Looked {
    /// The description to start from: the registry's, else the root Manifest's.
    described: Option<String>,
    manifests: Manifests,
    /// None when the repo was not looked at.
    outline: Option<Outline>,
}

impl Context<'_, '_> {
    /// The `ref` of `repo`. A registry add pins its package's version to a tag (`pin`), unless a
    /// `ref` was given with it: `expand` refused that when a version was too, and otherwise the
    /// `ref` is kept as it is. When nothing was pinned, or there was no registry package, the
    /// person is asked for one, if there is one to ask.
    fn settle_ref(
        &mut self,
        repo: &mut AddRepo,
        requested: Option<&Requested>,
    ) -> Result<(), Failure> {
        let mut ask_ref = requested.is_none();
        if let (None, Some(requested)) = (&repo.git_ref, requested) {
            let version = requested.version.as_deref();
            let package = &requested.package;
            let url = &repo.url;
            let prompter = self.asking.prompter();
            let pinned = pin(
                package,
                version,
                url,
                self.root,
                self.source,
                prompter,
                &mut *self.say,
            )?;
            repo.git_ref = match pinned {
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
            && let Some(prompter) = self.asking.prompter()
        {
            repo.git_ref = ask::ask_ref(prompter).map_err(Failure::Asked)?;
        }
        Ok(())
    }

    /// Look at the repo of `repo`, when there is a reason to: someone to ask, or `packages` to
    /// find. This comes before the rest of the questions, so that a bad URL or ref fails before
    /// they are asked and with nothing edited.
    fn look(
        &mut self,
        repo: &AddRepo,
        registry_description: Option<String>,
    ) -> Result<Looked, Failure> {
        let mut looked = Looked {
            described: registry_description,
            manifests: Manifests::default(),
            outline: None,
        };
        let source = self.source.filter(|_| {
            !repo.url.is_empty()
                && !self.offline
                && (self.asking.prompter.is_some() || repo.packages.is_empty())
        });
        let Some(source) = source else {
            return Ok(looked);
        };
        let git_ref = repo.git_ref.as_deref().unwrap_or("HEAD");
        let outline = source
            .outline(&repo.url, git_ref)
            .map_err(Failure::Source)?;
        // A registry add that names its package and describes it has no use for the Manifests.
        let nameless = repo.packages.is_empty();
        let undescribed = looked.described.is_none() && repo.description.is_none();
        if nameless || undescribed {
            looked.manifests =
                Manifests::read(source, &repo.url, &outline, &repo.paths, &mut *self.say)
                    .map_err(Failure::Source)?;
            looked.described = looked.described.or(looked.manifests.description());
        }
        looked.outline = Some(outline);
        Ok(looked)
    }
}
