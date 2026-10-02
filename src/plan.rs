//! The sync decisions, as pure functions (ADR 0003, 0004).

use std::collections::HashMap;

use crate::active::ActiveSet;
use crate::agent_file::splice;
use crate::config::RepoRef;
use crate::diagnostic::{LockRefusal, NotLocked, Note, Refusal};
use crate::lock::{Field, Lock, LockedRepo};
use crate::render::render;
use crate::source::{Observed, Pin};

/// One way the Lock differs from the active set, so a report can name the Repo and the
/// field (spec §10.2). `Changed` carries one entry per differing field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    LockMissing,
    Added(String),
    Removed(String),
    Changed { id: String, field: Field },
}

/// What the executor does for one active Repo. It verifies `paths` and `start` of
/// every Repo afterwards, so verification is not a variant (ADR 0004).
#[derive(Debug)]
pub enum Step<'a> {
    /// Ask the Source for a new pin: a new, re-enabled or changed Repo, or an upgrade.
    Resolve(RepoRef<'a>),
    /// Keep the locked pin; `paths` are already the config's.
    Reuse(LockedRepo),
}

#[derive(Debug)]
pub struct LockPlan<'a> {
    pub steps: Vec<Step<'a>>,
}

#[derive(Debug, Default)]
pub struct LockFlags {
    pub upgrade: Upgrade,
    pub offline: bool,
}

#[derive(Debug, Default)]
pub enum Upgrade {
    #[default]
    None,
    All,
    /// Only these Repos. An empty list upgrades nothing; the CLI maps a bare `--upgrade` to `All`.
    Ids(Vec<String>),
}

/// How the Lock differs from the active set; empty when it is current. `sync --check` uses
/// this alone, since it never resolves.
pub fn lock_drift(active: &ActiveSet, lock: Option<&Lock>) -> Vec<Drift> {
    let Some(lock) = lock else {
        return vec![Drift::LockMissing];
    };
    let mut drift = Vec::new();
    for repo in active.repos() {
        match lock.get(repo.id) {
            None => drift.push(Drift::Added(repo.id.into())),
            Some(entry) => {
                let mut fields = entry.pin.drift_from(repo.repo);
                if entry.paths != repo.repo.path_strings() {
                    fields.push(Field::Paths);
                }
                drift.extend(fields.into_iter().map(|field| Drift::Changed {
                    id: repo.id.into(),
                    field,
                }));
            }
        }
    }
    let is_active = |id: &str| active.get(id).is_some();
    drift.extend(
        lock.repo
            .iter()
            .filter(|e| !is_active(&e.id))
            .map(|e| Drift::Removed(e.id.clone())),
    );
    drift
}

/// Stage 1 (ADR 0004): one step per active Repo, in active-set order. `offline` refuses
/// anything that would resolve: a missing or stale Lock, or any `upgrade`.
pub fn plan_lock<'a>(
    active: &ActiveSet<'a>,
    lock: Option<&Lock>,
    flags: &LockFlags,
) -> Result<LockPlan<'a>, LockRefusal> {
    if let Upgrade::Ids(ids) = &flags.upgrade {
        let unknown: Vec<String> = ids
            .iter()
            .filter(|id| active.get(id).is_none())
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return Err(LockRefusal::UnknownUpgradeId { ids: unknown });
        }
    }
    if flags.offline && !matches!(flags.upgrade, Upgrade::None) {
        return Err(LockRefusal::OfflineUpgrade);
    }
    let drift = lock_drift(active, lock);
    if flags.offline && !drift.is_empty() {
        return Err(LockRefusal::OfflineStale { drift });
    }
    Ok(LockPlan {
        steps: active
            .repos()
            .map(|repo| match lock.and_then(|l| l.get(repo.id)) {
                Some(entry)
                    if entry.pin.drift_from(repo.repo).is_empty() && !flags.upgrades(repo) =>
                {
                    Step::Reuse(LockedRepo {
                        paths: repo.repo.path_strings(),
                        ..entry.clone()
                    })
                }
                _ => Step::Resolve(repo),
            })
            .collect(),
    })
}

impl LockFlags {
    fn upgrades(&self, repo: RepoRef) -> bool {
        repo.repo.is_floating()
            && match &self.upgrade {
                Upgrade::None => false,
                Upgrade::All => true,
                Upgrade::Ids(ids) => ids.iter().any(|id| id == repo.id),
            }
    }
}

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

/// What `sync` read of the project before stage 2: plain data, no trait.
#[derive(Debug, Clone)]
pub struct ProjectObserved {
    pub references_dir: String,
    pub agent_files: Vec<AgentFileText>,
    /// The directory names found in `references_dir`.
    pub listing: Vec<String>,
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
/// in the order removals, materialisations, Agent file writes, exclude rule. `observed`
/// holds every active Repo and every name in `project.listing`.
pub fn plan_checkouts(
    active: &ActiveSet,
    lock: &Lock,
    observed: &HashMap<String, Observed>,
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
        match observed.get(repo.id).unwrap_or(&Observed::Absent) {
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
    for name in project.listing.iter().filter(|n| active.get(n).is_none()) {
        match observed.get(name) {
            Some(Observed::At { dirty_files, .. }) if !dirty_files.is_empty() && !force => refusals
                .push(Action::Refuse(Refusal::DirtyCheckout {
                    id: name.clone(),
                    files: dirty_files.clone(),
                })),
            Some(Observed::At { .. }) => removals.push(Action::Remove { id: name.clone() }),
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

impl std::fmt::Display for Drift {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Drift::LockMissing => write!(f, "refs.lock is missing"),
            Drift::Added(id) => write!(f, "`{id}` is not locked"),
            Drift::Removed(id) => write!(f, "`{id}` is locked but no longer active"),
            Drift::Changed { id, field } => write!(f, "`{id}` changed its {field}"),
        }
    }
}
