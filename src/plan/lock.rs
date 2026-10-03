//! Stage 1 (ADR 0006): which Repos resolve and which keep their pin.

use crate::active::ActiveSet;
use crate::config::RepoRef;
use crate::diagnostic::LockRefusal;
use crate::lock::{Field, Lock, LockedRepo};

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
/// every Repo afterwards, so verification is not a variant (ADR 0006).
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

/// Stage 1 (ADR 0006): one step per active Repo, in active-set order. `offline` refuses
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
