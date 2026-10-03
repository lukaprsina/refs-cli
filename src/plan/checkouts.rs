//! Stage 2 (ADR 0006): what to do to the checkouts, the Agent files and the exclude rule.

use std::collections::BTreeMap;

use crate::active::ActiveSet;
use crate::agent_file::splice;
use crate::config::RepoRef;
use crate::diagnostic::{NotLocked, NotObserved, Note, Refusal};
use crate::lock::Lock;
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
/// other name found in the references directory. The one constructor checks that no active
/// Repo is missing, so an omitted entry cannot be read as `Absent`.
#[derive(Debug, Clone)]
pub struct Checkouts(BTreeMap<String, Observed>);

impl Checkouts {
    pub fn new(
        active: &ActiveSet,
        observed: impl IntoIterator<Item = (String, Observed)>,
    ) -> Result<Checkouts, NotObserved> {
        let observed: BTreeMap<String, Observed> = observed.into_iter().collect();
        let ids: Vec<String> = active
            .repos()
            .filter(|r| !observed.contains_key(r.id))
            .map(|r| r.id.to_string())
            .collect();
        if ids.is_empty() {
            Ok(Checkouts(observed))
        } else {
            Err(NotObserved { ids })
        }
    }

    fn of(&self, id: &str) -> &Observed {
        self.0.get(id).expect("`new` checked every active Repo")
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

/// What to do for one Repo's Checkout. One unit per Repo: a Repo's remove and materialise
/// are a single `Replace`, so nothing in the Plan depends on the order of two actions.
#[derive(Debug)]
pub enum RepoAction<'a> {
    /// Create or move the Checkout to `pin`; `Source` tells which from what is on disk.
    Materialise { repo: RepoRef<'a>, pin: Pin },
    /// Remove the Checkout, then materialise it: a Dangling one, or a dirty one under
    /// `--force`. `note` is reported once the whole action has succeeded.
    Replace {
        repo: RepoRef<'a>,
        pin: Pin,
        note: Option<Note>,
    },
    /// Remove the Checkout of a name that is no longer active. It takes an id because a
    /// Checkout can outlive its config entry.
    Remove { id: String },
}

impl RepoAction<'_> {
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
    /// Anything to do or refuse: everything except the note that there is no git repository.
    pub fn is_drift(&self) -> bool {
        !self.repos.is_empty()
            || !self.writes.is_empty()
            || !self.refusals.is_empty()
            || self.exclude == Some(ExcludeAction::Ensure)
    }
}

fn same_set(a: &[String], b: &[String]) -> bool {
    let sorted = |v: &[String]| {
        let mut v = v.to_vec();
        v.sort();
        v.dedup();
        v
    };
    sorted(a) == sorted(b)
}

/// Stage 2 (ADR 0006): what to do to the checkouts, the Agent files and the exclude rule.
pub fn plan_checkouts<'a>(
    active: &ActiveSet<'a>,
    lock: &Lock,
    checkouts: &Checkouts,
    project: &ProjectObserved,
    force: bool,
) -> Result<Plan<'a>, NotLocked> {
    let block = render(active, lock, &project.references_dir)?;
    let mut removals = Vec::new();
    let mut repos = Vec::new();
    let mut refusals = Vec::new();
    for repo in active.repos() {
        let locked = lock.get(repo.id).expect("render checked the Lock");
        let pin = || locked.pin.clone();
        match checkouts.of(repo.id) {
            Observed::Absent => repos.push(RepoAction::Materialise { repo, pin: pin() }),
            Observed::Dangling => repos.push(RepoAction::Replace {
                repo,
                pin: pin(),
                note: Some(Note::Recreated { id: repo.id.into() }),
            }),
            Observed::Foreign => refusals.push(Refusal::ForeignDir { id: repo.id.into() }),
            Observed::At {
                pin: seen, paths, ..
            } if seen.same_commit(&locked.pin) && same_set(paths, &repo.repo.path_strings()) => {}
            Observed::At { dirty_files, .. } if dirty_files.is_empty() => {
                repos.push(RepoAction::Materialise { repo, pin: pin() })
            }
            Observed::At { .. } if force => repos.push(RepoAction::Replace {
                repo,
                pin: pin(),
                note: None,
            }),
            Observed::At { dirty_files, .. } => refusals.push(Refusal::DirtyCheckout {
                id: repo.id.into(),
                files: dirty_files.clone(),
            }),
        }
    }
    // Names refs no longer manages. Only a checkout refs made (`At`) is touched; anything
    // else is for `doctor` to report.
    for (name, observed) in checkouts.unmanaged(active) {
        match observed {
            Observed::At { dirty_files, .. } if !dirty_files.is_empty() && !force => {
                refusals.push(Refusal::DirtyCheckout {
                    id: name.clone(),
                    files: dirty_files.clone(),
                })
            }
            Observed::At { .. } => removals.push(RepoAction::Remove { id: name.clone() }),
            _ => {}
        }
    }
    let mut writes = Vec::new();
    for file in &project.agent_files {
        match splice(file.text.as_deref().unwrap_or(""), &block) {
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
    removals.extend(repos);
    Ok(Plan {
        repos: removals,
        writes,
        exclude,
        refusals,
    })
}
