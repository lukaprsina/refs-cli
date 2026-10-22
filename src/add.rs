//! `refs add` (ADR 0008, 0009, 0011), all but the printing of its report (it says what it read and
//! decided, and warns, through a `Say`). First the repo it is asked to add is made ready: a
//! registry or forge shorthand expanded, its version pinned to a tag (`registry::pin`), the ref
//! asked for, and the repo looked at (`Source::outline`), so that a bad URL or ref fails before
//! the other questions. Its Manifests are read then, for the `packages` and the description.
//! Then the repo is added and synced once. With no sync (`--no-sync`) nothing is looked at, so
//! the `packages` stay as given.

use std::path::Path;

use crate::config::Config;
use crate::diagnostic::SourceError;
use crate::edit::{AddRepo, Edit};
use crate::packages::{infer, manifest_paths};
use crate::prompt::{self, Abort, Known, PackagesFor, Prompter};
use crate::registry::pin::{Say, Stop, pin};
use crate::registry::{self, Registry, RegistryError};
use crate::source::{Outline, Source};
use crate::sync::{self, Report, SyncFlags};

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
    /// The remote could not be looked at: a bad URL or ref, or its tags could not be listed.
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
    let repo = prepare(repo, source, root, config, asking, say)?;
    if asking.prompter.is_some() {
        say.status(format!("equivalent: {}", prompt::equivalent_command(&repo)));
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
    asking: &mut Asking,
    say: &mut dyn Say,
) -> Result<AddRepo, Failure> {
    let mut repo = repo.clone();
    if repo.url.is_empty()
        && let Some(prompter) = asking.prompter()
    {
        repo.url = prompt::ask_url(prompter).map_err(Failure::Asked)?;
    }
    let paths_given = !repo.paths.is_empty();
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
    // The directory the registry gave is where the paths picker starts, unlike one from `--paths`.
    let paths_proposed = ref_settled && !paths_given;
    let registry_description = expanded.description;
    let mut repo = expanded.repo;
    // A `ref` given with a version was refused by `expand`; without one it is kept as it is.
    if let (None, Some(requested)) = (&repo.git_ref, &expanded.requested) {
        let version = requested.version.as_deref();
        let package = &requested.package;
        let url = &repo.url;
        let prompter = asking.prompter();
        repo.git_ref = pin(package, version, url, root, source, prompter, say)?;
    }
    if repo.git_ref.is_none()
        && !ref_settled
        && let Some(prompter) = asking.prompter()
    {
        repo.git_ref = prompt::ask_ref(prompter).map_err(Failure::Asked)?;
    }
    // Looking at the repo comes before the rest of the questions, so that a bad URL or ref
    // fails before they are asked and with nothing edited.
    let looks = source.filter(|_| {
        !repo.url.is_empty()
            && !asking.offline
            && (asking.prompter.is_some() || repo.packages.is_empty())
    });
    let mut described = registry_description;
    let mut manifests = Vec::new();
    let mut outline = None;
    if let Some(source) = looks {
        let git_ref = repo.git_ref.as_deref().unwrap_or("HEAD");
        let looked = source
            .outline(&repo.url, git_ref)
            .map_err(Failure::Source)?;
        // A registry add that names its package and describes it has no use for the Manifests.
        if repo.packages.is_empty() || (described.is_none() && repo.description.is_none()) {
            manifests = read_manifests(source, &repo, &looked, say)?;
            described = described.or(infer(&manifests).description);
        }
        outline = Some(looked);
    }
    // Only the Manifests a Checkout of `paths` will hold count. None is fetched again.
    let packages_for = |paths: &[String]| {
        let read: Vec<String> = manifests.iter().map(|(path, _)| path.clone()).collect();
        let held = manifest_paths(&read, paths).0;
        let held: Vec<_> = manifests
            .iter()
            .filter(|(p, _)| held.contains(p))
            .cloned()
            .collect();
        infer(&held).packages
    };
    let Some(prompter) = asking.prompter() else {
        if repo.packages.is_empty() {
            repo.packages = packages_for(&repo.paths);
        }
        return match repo.url.is_empty() {
            true => Err(Failure::UrlRequired),
            false => Ok(repo),
        };
    };
    let known = Known {
        described: described.as_deref(),
        looked_at: outline.as_ref(),
        paths_proposed: paths_proposed && outline.is_some(),
        packages_for: outline.is_some().then_some(&packages_for as &PackagesFor),
    };
    prompt::fill_add(&mut repo, config, prompter, &known, say).map_err(Failure::Asked)?;
    Ok(repo)
}

/// The Manifests (path and text) of the `outline`d repo that `repo` will check out say. The Lock records
/// what the sync resolves, which is another commit than this one if upstream moved in between:
/// accepted.
fn read_manifests(
    source: &dyn Source,
    repo: &AddRepo,
    outline: &Outline,
    say: &mut dyn Say,
) -> Result<Vec<(String, String)>, Failure> {
    let (chosen, left) = manifest_paths(&outline.paths, &repo.paths);
    if left > 0 {
        let read = chosen.len();
        say.status(format!("read {read} Manifests and left {left} unread"));
    }
    source
        .read(&repo.url, &outline.pin, &chosen)
        .map_err(Failure::Source)
}
