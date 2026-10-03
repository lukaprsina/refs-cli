//! Stage 2 (ADR 0006): what to do to the checkouts, the Agent files and the exclude rule.

use std::collections::BTreeMap;

use crate::active::ActiveSet;
use crate::agent_file::splice;
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

#[derive(Debug)]
pub enum Action {
    Remove {
        id: String,
    },
    /// Create or move the Checkout to `pin`; `Source` tells which from what is on disk.
    Materialise {
        id: String,
        pin: Pin,
    },
    /// Replace the file with `text` (the whole file, block spliced in).
    WriteAgentFile {
        path: String,
        text: String,
    },
    EnsureExclude,
    Refuse(Refusal),
    Note(Note),
}

#[derive(Debug)]
pub struct Plan {
    pub actions: Vec<Action>,
}

impl Plan {
    /// Anything to do or refuse: every action except a `Note` counts.
    pub fn is_drift(&self) -> bool {
        self.actions.iter().any(|a| !matches!(a, Action::Note(_)))
    }

    pub fn refusals(&self) -> impl Iterator<Item = &Refusal> {
        self.actions.iter().filter_map(|a| match a {
            Action::Refuse(r) => Some(r),
            _ => None,
        })
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

/// Stage 2 (ADR 0004): what to do to the checkouts, the Agent files and the exclude rule,
/// in the order removals, materialisations, Agent file writes, exclude rule.
pub fn plan_checkouts(
    active: &ActiveSet,
    lock: &Lock,
    checkouts: &Checkouts,
    project: &ProjectObserved,
    force: bool,
) -> Result<Plan, NotLocked> {
    let block = render(active, lock, &project.references_dir)?;
    let mut removals = Vec::new();
    let mut materialisations = Vec::new();
    let mut refusals = Vec::new();
    for repo in active.repos() {
        let locked = lock.get(repo.id).expect("render checked the Lock");
        let id = || repo.id.to_string();
        let materialise = Action::Materialise {
            id: id(),
            pin: locked.pin.clone(),
        };
        match checkouts.of(repo.id) {
            Observed::Absent => materialisations.push(materialise),
            Observed::Dangling => {
                removals.push(Action::Remove { id: id() });
                materialisations.push(materialise);
                materialisations.push(Action::Note(Note::Recreated { id: id() }));
            }
            Observed::Foreign => refusals.push(Action::Refuse(Refusal::ForeignDir { id: id() })),
            Observed::At { pin, paths, .. }
                if pin.same_commit(&locked.pin) && same_set(paths, &repo.repo.path_strings()) => {}
            Observed::At { dirty_files, .. } if dirty_files.is_empty() => {
                materialisations.push(materialise)
            }
            Observed::At { .. } if force => {
                removals.push(Action::Remove { id: id() });
                materialisations.push(materialise);
            }
            Observed::At { dirty_files, .. } => {
                refusals.push(Action::Refuse(Refusal::DirtyCheckout {
                    id: id(),
                    files: dirty_files.clone(),
                }))
            }
        }
    }
    // Names refs no longer manages. Only a checkout refs made (`At`) is touched; anything
    // else is for `doctor` to report.
    for (name, observed) in checkouts.unmanaged(active) {
        match observed {
            Observed::At { dirty_files, .. } if !dirty_files.is_empty() && !force => {
                refusals.push(Action::Refuse(Refusal::DirtyCheckout {
                    id: name.clone(),
                    files: dirty_files.clone(),
                }))
            }
            Observed::At { .. } => removals.push(Action::Remove { id: name.clone() }),
            _ => {}
        }
    }
    let mut writes = Vec::new();
    for file in &project.agent_files {
        match splice(file.text.as_deref().unwrap_or(""), &block) {
            Ok(text) if file.text.as_deref() != Some(text.as_str()) => {
                writes.push(Action::WriteAgentFile {
                    path: file.path.clone(),
                    text,
                })
            }
            Ok(_) => {}
            Err(error) => refusals.push(Action::Refuse(Refusal::Block {
                path: file.path.clone(),
                error,
            })),
        }
    }
    // A block listing a Repo with no checkout is the harm, so nothing is written past a
    // refusal. The exclude rule is independent of it.
    if !refusals.is_empty() {
        writes.clear();
    }
    let exclude = match project.exclude {
        Exclude::Present => None,
        Exclude::Missing => Some(Action::EnsureExclude),
        Exclude::NoGit => Some(Action::Note(Note::NoGitRepo)),
    };
    let actions: Vec<Action> = removals
        .into_iter()
        .chain(materialisations)
        .chain(refusals)
        .chain(writes)
        .chain(exclude)
        .collect();
    Ok(Plan { actions })
}
