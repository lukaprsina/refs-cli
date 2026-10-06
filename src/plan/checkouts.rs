//! Stage 2 (ADR 0006): what to do to the checkouts, the Agent files and the exclude rule.

use std::collections::BTreeMap;

use crate::active::ActiveSet;
use crate::agent_file::{splice, strip};
use crate::config::RepoRef;
use crate::diagnostic::{Note, Refusal};
use crate::lock::Lock;
use crate::plan::classify::{CheckoutState, classify};
use crate::plan::outcome::Outcome;
use crate::render::render;
use crate::source::{Observed, Pin};

/// The text of one Agent file, `None` when it does not exist.
#[derive(Debug, Clone)]
pub struct AgentFileText {
    pub path: String,
    pub text: Option<String>,
}

/// Whether the project's exclude rule for `references_dir` is in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclude {
    Present,
    Missing,
    /// Not a git repository, so there is nowhere to put the rule.
    NoGit,
}

/// What `inspect` found for every Repo stage 2 has to judge: each active Repo, and each
/// other name found in the references directory.
#[derive(Debug, Clone)]
pub struct Checkouts(BTreeMap<String, Observed>);

impl Checkouts {
    /// Inspect every active Repo, then each name in `listing` (what `Source::list` found in
    /// the references directory) that is not active. The one constructor decides what is
    /// observed and runs `inspect` itself, so an active Repo cannot be left out and read as
    /// `Absent`. Every failure is collected.
    pub fn observe<E>(
        active: &ActiveSet,
        listing: &[String],
        mut inspect: impl FnMut(&str) -> Result<Observed, E>,
    ) -> Result<Checkouts, Vec<E>> {
        let names = active
            .repos()
            .map(|r| r.id.to_string())
            .chain(listing.iter().filter(|n| active.get(n).is_none()).cloned());
        let mut seen = BTreeMap::new();
        let mut errors = Vec::new();
        for name in names {
            match inspect(&name) {
                Ok(observed) => {
                    seen.insert(name, observed);
                }
                Err(e) => errors.push(e),
            }
        }
        if errors.is_empty() {
            Ok(Checkouts(seen))
        } else {
            Err(errors)
        }
    }

    /// What `observe` found for `id`: `None` for a name it did not inspect.
    pub fn get(&self, id: &str) -> Option<&Observed> {
        self.0.get(id)
    }

    fn of(&self, id: &str) -> &Observed {
        self.0
            .get(id)
            .expect("`observe` inspected every active Repo")
    }

    /// The names in the references directory that are not active Repos.
    fn unmanaged<'a>(
        &'a self,
        active: &'a ActiveSet,
    ) -> impl Iterator<Item = (&'a String, &'a Observed)> {
        self.0.iter().filter(|(name, _)| active.get(name).is_none())
    }
}

/// What `sync` read of the project before stage 2: plain data, no trait.
#[derive(Debug, Clone)]
pub struct ProjectObserved {
    pub references_dir: String,
    pub agent_files: Vec<AgentFileText>,
    pub exclude: Exclude,
}

/// How a Checkout comes to be at a Pin. The two that remove first are one action with the
/// materialise, so nothing in the Plan depends on the order of two actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Nothing is there.
    Create,
    /// Move a clean Checkout to the Pin.
    Move,
    /// Remove a Dangling Checkout and create it again; the run says so.
    Recreate,
    /// Remove a dirty Checkout (only under `--force`) and create it at the Pin.
    Overwrite,
}

impl Placement {
    /// Whether the existing Checkout is removed before the materialise.
    pub fn removes_first(self) -> bool {
        matches!(self, Placement::Recreate | Placement::Overwrite)
    }
}

/// What to do for one Repo's Checkout. One unit per Repo.
#[derive(Debug)]
pub enum RepoAction<'a> {
    /// Put the Repo's Checkout at `pin`.
    Materialise {
        repo: RepoRef<'a>,
        pin: Pin,
        placement: Placement,
    },
    /// Remove the Checkout of a name that is not Active, for example a disabled or deleted
    /// Repo. It takes an id because a Checkout can outlive its config entry.
    Remove { id: String },
}

/// What an action does to a Checkout, for the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkout {
    Created,
    Moved,
    Removed,
}

impl RepoAction<'_> {
    /// The Repo the action is for.
    pub fn id(&self) -> &str {
        match self {
            RepoAction::Materialise { repo, .. } => repo.id,
            RepoAction::Remove { id } => id,
        }
    }

    pub fn how(&self) -> Checkout {
        match self {
            RepoAction::Materialise { placement, .. } => match placement {
                Placement::Create | Placement::Recreate => Checkout::Created,
                Placement::Move | Placement::Overwrite => Checkout::Moved,
            },
            RepoAction::Remove { .. } => Checkout::Removed,
        }
    }

    /// What the run says once the whole action has succeeded.
    pub fn note(&self) -> Option<Note> {
        match self {
            RepoAction::Materialise {
                repo,
                placement: Placement::Recreate,
                ..
            } => Some(Note::Recreated { id: repo.id.into() }),
            _ => None,
        }
    }

    /// Whether the block lists this Repo's Checkout, so that the block must not be written
    /// if the action failed. A Checkout that is only being removed is not listed.
    pub fn gates_writes(&self) -> bool {
        !matches!(self, RepoAction::Remove { .. })
    }
}

/// Replace the file with `text` (the whole file, block spliced in).
#[derive(Debug)]
pub struct WriteAgentFile {
    pub path: String,
    pub text: String,
}

/// What the plan does about the exclude rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludeAction {
    Ensure,
    /// Not a git repository: say so, there is nothing to do.
    NoGit,
}

/// Stage 2's decisions, by what they act on (ADR 0006). The executor runs `repos` (a
/// failure of one does not stop the others), then reports `refusals`, then runs `writes`
/// only if no `gates_writes` action failed, each independently, then the exclude rule.
#[derive(Debug)]
pub struct Plan<'a> {
    /// Removals of non-active names first, then the active Repos in block order.
    pub repos: Vec<RepoAction<'a>>,
    pub writes: Vec<WriteAgentFile>,
    pub exclude: Option<ExcludeAction>,
    pub refusals: Vec<Refusal>,
}

impl Plan<'_> {
    /// What applying the Plan comes to if nothing the executor does fails: it refused or it
    /// did not.
    pub fn applied_outcome(&self) -> Outcome {
        if self.refusals.is_empty() {
            Outcome::InSync
        } else {
            Outcome::Refused
        }
    }

    /// Whether the Agent file writes are held back, given the indices into `repos` of the
    /// actions that failed: they are if one of them is on a Checkout the block lists.
    pub fn holds_back_writes(&self, failed: &[usize]) -> bool {
        failed.iter().any(|&i| self.repos[i].gates_writes())
    }

    /// Anything to do or refuse: everything except the note that there is no git repository.
    pub fn is_drift(&self) -> bool {
        !self.repos.is_empty()
            || !self.writes.is_empty()
            || !self.refusals.is_empty()
            || self.exclude == Some(ExcludeAction::Ensure)
    }
}

fn place(repo: RepoRef, pin: Pin, placement: Placement) -> RepoAction {
    RepoAction::Materialise {
        repo,
        pin,
        placement,
    }
}

fn dirty(id: &str, files: &[String]) -> Refusal {
    Refusal::DirtyCheckout {
        id: id.into(),
        files: files.to_vec(),
    }
}

/// Plan stage 2 against the Lock and what is on disk.
pub fn plan_checkouts<'a>(
    active: &ActiveSet<'a>,
    lock: &Lock,
    checkouts: &Checkouts,
    project: &ProjectObserved,
    force: bool,
) -> Plan<'a> {
    // With no active Repo there is nothing to list: the block goes, markers included.
    let block = if active.repos().next().is_some() {
        Some(render(active, lock, &project.references_dir))
    } else {
        None
    };
    let mut stale = Vec::new();
    let mut current = Vec::new();
    let mut refusals = Vec::new();
    for repo in active.repos() {
        let locked = lock.get(repo.id).expect("render checked the Lock");
        let pin = || locked.pin.clone();
        match classify(checkouts.of(repo.id), repo.repo, &locked.pin) {
            CheckoutState::InSync => {}
            CheckoutState::Absent => current.push(place(repo, pin(), Placement::Create)),
            CheckoutState::Dangling => current.push(place(repo, pin(), Placement::Recreate)),
            CheckoutState::Foreign => refusals.push(Refusal::ForeignDir { id: repo.id.into() }),
            CheckoutState::Stale { dirty_files, .. } if dirty_files.is_empty() => {
                current.push(place(repo, pin(), Placement::Move))
            }
            CheckoutState::Stale { .. } if force => {
                current.push(place(repo, pin(), Placement::Overwrite))
            }
            CheckoutState::Stale { dirty_files, .. } => refusals.push(dirty(repo.id, &dirty_files)),
        }
    }
    // Names refs no longer manages. Only a checkout refs made (`At`) is touched; anything
    // else is for `doctor` to report.
    for (name, observed) in checkouts.unmanaged(active) {
        match observed {
            Observed::At { dirty_files, .. } if !dirty_files.is_empty() && !force => {
                refusals.push(dirty(name, dirty_files))
            }
            Observed::At { .. } => stale.push(RepoAction::Remove { id: name.clone() }),
            _ => {}
        }
    }
    let mut writes = Vec::new();
    for file in &project.agent_files {
        let current = file.text.as_deref().unwrap_or("");
        let planned = match &block {
            Some(block) => splice(current, block),
            None => strip(current),
        };
        match planned {
            // an absent file with no block to add is not created
            Ok(text) if file.text.is_none() && text.is_empty() => {}
            Ok(text) if file.text.as_deref() != Some(text.as_str()) => {
                writes.push(WriteAgentFile {
                    path: file.path.clone(),
                    text,
                })
            }
            Ok(_) => {}
            Err(error) => refusals.push(Refusal::Block {
                path: file.path.clone(),
                error,
            }),
        }
    }
    // A block listing a Repo with no checkout is the harm, so nothing is written past a
    // refusal. The exclude rule is independent of it.
    if !refusals.is_empty() {
        writes.clear();
    }
    let exclude = match project.exclude {
        Exclude::Present => None,
        Exclude::Missing => Some(ExcludeAction::Ensure),
        Exclude::NoGit => Some(ExcludeAction::NoGit),
    };
    Plan {
        repos: stale.into_iter().chain(current).collect(),
        writes,
        exclude,
        refusals,
    }
}
