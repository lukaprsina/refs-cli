//! `refs add` (ADR 0008, 0009), all but the printing of its reports (it says what it read and decided, and warns, through a `Say`). First the repo it is asked
//! to add is made ready: a registry or forge shorthand expanded, its version pinned to a tag
//! (`registry::pin`), and what the command line left out asked for. Then the repo is added and
//! synced, the new Checkout's Manifests are read for its `packages`, the person confirms them,
//! and a second sync that only re-renders the Managed block writes them. With no sync
//! (`--no-sync`) there is no Checkout, so the `packages` stay as given. What is read and decided
//! on the way is said through a `Say`.

use std::path::Path;

use crate::config::Config;
use crate::diagnostic::SourceError;
use crate::edit::{AddRepo, Edit};
use crate::packages::infer_packages;
use crate::prompt::{self, Abort, Prompter};
use crate::registry::pin::{Say, Stop, pin};
use crate::registry::{self, Registry, RegistryError};
use crate::source::Source;
use crate::sync::{self, Outcome, Report, SyncFlags};

/// Who `add` can ask and what it can ask about.
pub struct Asking<'a> {
    /// The person to ask, when stdin and stderr are terminals and `--no-input` is not set.
    pub prompter: Option<&'a mut dyn Prompter>,
    pub registry: &'a dyn Registry,
    pub offline: bool,
}

impl Asking<'_> {
    pub fn prompter(&mut self) -> Option<&mut dyn Prompter> {
        self.prompter
            .as_mut()
            .map(|p| &mut **p as &mut dyn Prompter)
    }
}

/// Why `add` stopped before it edited anything.
#[derive(Debug)]
pub enum Failure {
    Registry(RegistryError),
    /// The tags of the remote could not be listed.
    Source(SourceError),
    /// A question ended without an answer.
    Asked(Abort),
    /// A question had to be asked and there is no one to ask: the message says what to do.
    Refused(String),
    /// There is no URL and no one to ask for it.
    UrlRequired,
}

impl From<Stop> for Failure {
    fn from(stop: Stop) -> Failure {
        match stop {
            Stop::Source(e) => Failure::Source(e),
            Stop::Asked(abort) => Failure::Asked(abort),
            Stop::Refused(why) => Failure::Refused(why),
        }
    }
}

/// What an `add` did, for the CLI to print.
#[derive(Debug)]
pub struct Added {
    /// The add and its sync, then the sync that wrote `packages` if there was one.
    pub reports: Vec<Report>,
    /// The repo as it ended up in `refs.toml`: the one given, with the `packages` the second
    /// sync wrote if there was one.
    pub repo: AddRepo,
    /// Why the repo was left without the `packages` it was asked for, if it was.
    pub skipped: Option<Skipped>,
    /// Whether there was someone to ask, so the equivalent command line is worth printing.
    pub asked: bool,
}

/// Why `add` left a repo without `packages` after asking for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skipped {
    /// The person gave up the question or answered it with nothing.
    Declined,
    /// The terminal failed.
    PromptFailed(String),
}

/// Add `repo`, as the command line gave it, to the project at `root` and sync it. It is first
/// made ready (see `prepare`). Unless it names its `packages`, they are then read from the
/// Checkout: asked for with the names found as the default when there is a prompter, otherwise
/// the names found are written. `flags` are for the add's own sync. Without a `source`
/// (`--no-sync`) the repo is only added: there are no tags to list and no Checkout to read, so
/// the version is not pinned and the `packages` stay as given.
pub fn run(
    source: Option<&dyn Source>,
    root: &Path,
    config: &Config,
    repo: &AddRepo,
    flags: &SyncFlags,
    asking: &mut Asking,
    say: &mut dyn Say,
) -> Result<Added, Failure> {
    let repo = prepare(repo, source, root, config, asking, say)?;
    let mut added = sync_and_infer(source, root, config, &repo, flags, asking);
    added.asked = asking.prompter.is_some();
    Ok(added)
}

/// The add of the prepared `repo` and its sync, then the `packages` (see `run`).
fn sync_and_infer(
    source: Option<&dyn Source>,
    root: &Path,
    config: &Config,
    repo: &AddRepo,
    flags: &SyncFlags,
    asking: &mut Asking,
) -> Added {
    let edit = Edit::Add(repo);
    let first = match source {
        Some(source) => sync::edit(source, root, &edit, flags),
        None => sync::edit_only(root, &edit),
    };
    let mut added = Added {
        reports: Vec::new(),
        repo: repo.clone(),
        skipped: None,
        asked: false,
    };
    let infer = repo.packages.is_empty() && first.outcome == Outcome::InSync;
    added.reports.push(first);
    let Some(source) = source.filter(|_| infer) else {
        return added;
    };
    let id = repo.resolved_id();
    let checkout = root.join(config.settings.references_dir()).join(&id);
    let inferred = infer_packages(&checkout);
    let prompter = asking.prompter();
    let prompted = prompter.is_some();
    let names = match prompter {
        Some(prompter) => match prompt::ask_packages(&inferred, prompter) {
            Ok(names) => names,
            Err(Abort::Cancelled) => return added.skip(Skipped::Declined),
            Err(Abort::Failed(why)) => return added.skip(Skipped::PromptFailed(why)),
        },
        None => inferred,
    };
    if names.is_empty() && prompted {
        return added.skip(Skipped::Declined);
    }
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
        added.repo.packages = names;
    }
    added
}

/// `repo` ready to add: a registry shorthand expanded and its version pinned to a tag, then
/// what the command line left out asked for. The version is the one given, else the one the
/// project's Package lockfile at `root` says it uses. The tags come from `source`. Only a
/// missing URL is a problem without a prompter. On a cancel nothing has been edited.
fn prepare(
    repo: &AddRepo,
    source: Option<&dyn Source>,
    root: &Path,
    config: &Config,
    asking: &mut Asking,
    say: &mut dyn Say,
) -> Result<AddRepo, Failure> {
    let mut repo = repo.clone();
    if repo.url.is_empty()
        && let Some(prompter) = asking.prompter()
    {
        repo.url = prompt::ask_url(prompter).map_err(Failure::Asked)?;
    }
    let expanded =
        registry::expand(&repo, asking.registry, asking.offline).map_err(Failure::Registry)?;
    if let Some(ignored) = &expanded.ignored {
        let shorthand = &repo.url;
        say.warning(format!(
            "ignored `{ignored}` after the repository in `{shorthand}`; \
             use `--ref` and `--paths` to choose a ref and directories"
        ));
    }
    // A registry add has chosen its Ref (a tag, or the default branch), so it is not asked.
    let ref_settled = expanded.requested.is_some();
    let mut repo = expanded.repo;
    // A `ref` given with a version was refused by `expand`; without one it is kept as it is.
    if let (None, Some(requested)) = (&repo.git_ref, &expanded.requested) {
        let version = requested.version.as_deref();
        let package = &requested.package;
        let url = &repo.url;
        let prompter = asking.prompter();
        repo.git_ref = pin(package, version, url, root, source, prompter, say)?;
    }
    let Some(prompter) = asking.prompter() else {
        return match repo.url.is_empty() {
            true => Err(Failure::UrlRequired),
            false => Ok(repo),
        };
    };
    prompt::fill_add(&mut repo, config, prompter, ref_settled).map_err(Failure::Asked)?;
    Ok(repo)
}

impl Added {
    fn skip(mut self, why: Skipped) -> Added {
        self.skipped = Some(why);
        self
    }
}
