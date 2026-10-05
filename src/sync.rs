//! The `sync` executor (ADR 0006): reads the project, runs the typed actions of the two
//! plans through `Source`, and collects what happened. Which names are observed, how Lock
//! entries are built, and how a Plan becomes an outcome are decided in `plan`.

use std::path::Path;

use crate::active::{ActiveSet, active};
use crate::agent_file;
use crate::config::{self, Config};
use crate::diagnostic::{NotLocked, Note, SourceError};
use crate::edit::Edit;
use crate::lock::Lock;
pub use crate::plan::{Checkout, Outcome};
use crate::plan::{
    Checkouts, Coverage, Drift, ExcludeAction, Failure, Keep, LockFlags, Plan, RepoAction, Settled,
    Step, check_outcome, conclude, lock_drift, locked, plan_checkouts, plan_lock, settle,
};
use crate::project;
use crate::source::{MaterialiseOpts, Source, VerifyOpts};

#[derive(Debug, Default)]
pub struct SyncFlags {
    pub offline: bool,
    pub force: bool,
    pub check: bool,
}

/// Something a run changed on disk, for the CLI to say. `refs.toml` is not here: only `edit`
/// writes it, and it says so in `Edited::change`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// `refs.lock` was written.
    Lock,
    /// The Checkout of this Repo was created, moved or removed.
    Checkout { id: String, how: Checkout },
    /// This Agent file was written (path relative to the Project).
    AgentFile(String),
}

#[derive(Debug)]
pub struct Report {
    pub outcome: Outcome,
    /// What the run changed on disk, in the order it happened.
    pub changes: Vec<Changed>,
    /// How the Lock differed from the config when the run began.
    pub drift: Vec<Drift>,
    /// Refusals, failures and notes, in the order they happened.
    pub diagnostics: Vec<miette::Report>,
}

impl Report {
    fn new(outcome: Outcome, drift: Vec<Drift>, diagnostics: Vec<miette::Report>) -> Report {
        Report {
            outcome,
            changes: Vec::new(),
            drift,
            diagnostics,
        }
    }

    fn failed(error: impl miette::Diagnostic + Send + Sync + 'static) -> Report {
        Report::new(Outcome::Failed, vec![], vec![miette::Report::new(error)])
    }
}

/// Stage 1 as it left things: the Lock stage 2 plans against (with `--check`, the one on
/// disk), whether it was written, and the Repos' errors (empty unless an edit that only
/// shrinks the active set went on past them).
struct StageOne {
    lock: Lock,
    wrote_lock: bool,
    errors: Vec<miette::Report>,
}

/// Stage 1 stopped the run: the outcome and every diagnostic.
struct Halt {
    outcome: Outcome,
    diagnostics: Vec<miette::Report>,
}

impl Halt {
    fn failed(error: impl miette::Diagnostic + Send + Sync + 'static) -> Halt {
        Halt {
            outcome: Outcome::Failed,
            diagnostics: vec![miette::Report::new(error)],
        }
    }

    fn into_report(self, drift: Vec<Drift>) -> Report {
        Report::new(self.outcome, drift, self.diagnostics)
    }
}

/// What stage 2 did or found, before it is combined with stage 1's.
struct StageTwo {
    outcome: Outcome,
    diagnostics: Vec<miette::Report>,
    changes: Vec<Changed>,
}

impl StageTwo {
    fn only(outcome: Outcome) -> StageTwo {
        StageTwo {
            outcome,
            diagnostics: Vec::new(),
            changes: Vec::new(),
        }
    }
}

/// The Report of a run that got through stage 1: `plan::conclude` decides the outcome and the
/// order of the diagnostics, and the Lock write came before everything stage 2 did.
fn finish(stage_one: StageOne, drift: Vec<Drift>, stage_two: StageTwo) -> Report {
    let concluded = conclude(stage_one.errors, stage_two.outcome, stage_two.diagnostics);
    let mut changes = stage_two.changes;
    if stage_one.wrote_lock {
        changes.insert(0, Changed::Lock);
    }
    Report {
        outcome: concluded.outcome,
        changes,
        drift,
        diagnostics: concluded.diagnostics,
    }
}

/// What every run starts from: the active set, the Lock on disk (if any) and how it differs.
struct Preflight<'a> {
    active: ActiveSet<'a>,
    old: Option<Lock>,
    drift: Vec<Drift>,
}

fn preflight<'a>(root: &Path, config: &'a Config) -> Result<Preflight<'a>, Report> {
    let active = active(config);
    let old = Lock::read(&Lock::path(root)).map_err(Report::failed)?;
    let drift = lock_drift(&active, old.as_ref());
    Ok(Preflight { active, old, drift })
}

/// `refs lock`: stage 1 and a write.
pub fn lock(source: &dyn Source, root: &Path, config: &Config, flags: &LockFlags) -> Report {
    let Preflight { active, old, drift } = match preflight(root, config) {
        Ok(preflight) => preflight,
        Err(report) => return report,
    };
    match stage_one(source, &active, old, flags, root) {
        Ok(settled) => {
            let stage_one = StageOne {
                lock: settled.lock,
                wrote_lock: settled.write_lock,
                errors: vec![],
            };
            finish(stage_one, drift, StageTwo::only(Outcome::InSync))
        }
        Err(halt) => halt.into_report(drift),
    }
}

/// `refs sync`. With `check`, nothing is resolved, verified, fetched or written.
pub fn sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    let Preflight { active, old, drift } = match preflight(root, config) {
        Ok(preflight) => preflight,
        Err(report) => return report,
    };
    let (lock, wrote_lock) = if flags.check {
        match old {
            Some(lock) => (lock, false),
            // Nothing to plan against: the Lock is what is out of date.
            None => {
                let stage_two = StageTwo::only(check_outcome(None, true));
                let stage_one = StageOne {
                    lock: Lock::new(vec![]),
                    wrote_lock: false,
                    errors: vec![],
                };
                return finish(stage_one, drift, stage_two);
            }
        }
    } else {
        match stage_one(source, &active, old, &lock_flags(flags), root) {
            Ok(settled) => (settled.lock, settled.write_lock),
            Err(halt) => return halt.into_report(drift),
        }
    };
    let coverage = Coverage::full(active);
    let stage_one = StageOne {
        lock,
        wrote_lock,
        errors: vec![],
    };
    plan_and_apply(source, root, config, &coverage, stage_one, drift, flags)
}

/// Stage 1 as `sync` runs it: only `offline` carries over.
fn lock_flags(flags: &SyncFlags) -> LockFlags {
    LockFlags {
        offline: flags.offline,
        ..LockFlags::default()
    }
}

/// Stage 2 against a Lock that stage 1 has just produced (or, with `--check`, the one on
/// disk): plan the checkouts, then report on the plan or apply it.
fn plan_and_apply(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    coverage: &Coverage,
    stage_one: StageOne,
    drift: Vec<Drift>,
    flags: &SyncFlags,
) -> Report {
    let plan = match plan_stage_two(source, root, config, coverage, &stage_one.lock, flags.force) {
        Ok(plan) => plan,
        // Only `--check` can meet a Lock that does not cover the active set, and it does not
        // repair it: the Lock is what is out of date.
        Err(StageTwoError::NotLocked(_)) if flags.check => {
            let stage_two = StageTwo::only(check_outcome(None, true));
            return finish(stage_one, drift, stage_two);
        }
        Err(e) => {
            let stage_two = StageTwo {
                diagnostics: e.into_reports(),
                ..StageTwo::only(Outcome::Failed)
            };
            return finish(stage_one, drift, stage_two);
        }
    };
    let stage_two = if flags.check {
        check(plan, !drift.is_empty())
    } else {
        let dir = config.settings.references_dir();
        apply(source, root, dir, plan, flags)
    };
    finish(stage_one, drift, stage_two)
}

/// What happened to `refs.toml` in `edit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The edit gave the text it already had; nothing was written.
    Unchanged,
    /// `refs.toml` was written.
    Written,
    /// The edit or stage 1 failed against it: nothing was written.
    Rejected,
}

/// The result of `edit`: what happened to `refs.toml`, and what followed.
#[derive(Debug)]
pub struct Edited {
    pub change: Change,
    /// The Group the edit created (`add --group x` for a Group that was missing).
    pub group_created: Option<String>,
    /// The problems of the edit and of the sync that followed it.
    pub report: Report,
}

fn rejected(report: Report) -> Edited {
    Edited {
        change: Change::Rejected,
        group_created: None,
        report,
    }
}

/// Apply `edit` to `<root>/refs.toml` and, unless `no_sync`, sync the project to it. The
/// edited text is parsed once, here, into the config the sync runs against; `make_source` is asked for a `Source` only if there is a sync.
///
/// Stage 1 runs against the edited config, and `refs.toml` is written only if it passes
/// (stage 1 writes the Lock), then stage 2 runs on the Lock stage 1 produced, so each Repo
/// is verified once. A stage 2 failure keeps the edit and the Lock; `refs sync` retries it. When `remove` or `disable` leaves a Repo that cannot be locked, stage 2 still runs for the Repos that did lock (the Checkouts and the block follow the partial Lock) and leaves the failed Repo's Checkout alone. If every remaining Repo cannot be locked, stage 2 refuses to leave the stale block (`Refusal::StaleBlock`).
/// An edit that changes nothing writes nothing and just syncs. `flags.check` is not for
/// this function: it writes. The Lock is written before `refs.toml`, so a failed write of
/// `refs.toml` leaves a Lock with an entry the config does not have, which the next `sync`
/// removes.
pub fn edit<S: Source>(
    root: &Path,
    edit: &Edit,
    no_sync: bool,
    make_source: impl FnOnce() -> Result<S, SourceError>,
    flags: &SyncFlags,
) -> Edited {
    let before = match project::read_config(root) {
        Ok(text) => text,
        Err(e) => return rejected(Report::failed(e)),
    };
    let applied = match edit.apply(&before) {
        Ok(applied) => applied,
        Err(e) => return rejected(Report::failed(e)),
    };
    let after = applied.text;
    let config = match config::parse(&after) {
        Ok(config) => config,
        Err(e) => return rejected(Report::failed(e)),
    };
    let edited = |change, report| Edited {
        change,
        group_created: applied.group_created.clone(),
        report,
    };
    let unchanged = after == before;
    if no_sync {
        let change = if unchanged {
            Change::Unchanged
        } else {
            match project::write_config(root, &after) {
                Ok(()) => Change::Written,
                Err(e) => return rejected(Report::failed(e)),
            }
        };
        return edited(change, Report::new(Outcome::InSync, vec![], vec![]));
    }
    let source = match make_source() {
        Ok(source) => source,
        Err(e) => return rejected(Report::failed(e)),
    };
    if unchanged {
        return edited(Change::Unchanged, sync(&source, root, &config, flags));
    }
    let Preflight { active, old, drift } = match preflight(root, &config) {
        Ok(preflight) => preflight,
        Err(report) => return rejected(report),
    };
    let keep = if edit.grows() {
        Keep::AllOrNothing
    } else {
        Keep::Passing
    };
    let settled = match run_stage_one(&source, &active, old, &lock_flags(flags), root, keep) {
        Ok(settled) => settled,
        Err(halt) => return rejected(halt.into_report(drift)),
    };
    if !settled.accepted {
        return rejected(Report::new(Outcome::Failed, drift, settled.errors));
    }
    if let Err(e) = project::write_config(root, &after) {
        return rejected(Report::failed(e));
    }
    // The Lock covers the Repos that passed. Stage 2 runs for those, so the Checkouts and
    // the block drop what the edit removed; a Repo that failed keeps its Checkout and is
    // reported, and `refs sync` finishes it once it is fixed.
    let stage_one = StageOne {
        lock: settled.lock,
        wrote_lock: settled.write_lock,
        errors: settled.errors,
    };
    let report = plan_and_apply(
        &source,
        root,
        &config,
        &settled.coverage,
        stage_one,
        drift,
        flags,
    );
    edited(Change::Written, report)
}

/// Stage 1 for `lock` and `sync`: all or nothing. A failure of any Repo fails the run with
/// every error and writes nothing.
fn stage_one<'a>(
    source: &dyn Source,
    active: &ActiveSet<'a>,
    old: Option<Lock>,
    flags: &LockFlags,
    root: &Path,
) -> Result<Settled<'a>, Halt> {
    let settled = run_stage_one(source, active, old, flags, root, Keep::AllOrNothing)?;
    if settled.accepted {
        Ok(settled)
    } else {
        Err(Halt {
            outcome: Outcome::Failed,
            diagnostics: settled.errors,
        })
    }
}

/// Resolve or reuse every Repo and verify them all, collecting every error; `settle` decides
/// what the failures hold back, and the Lock is written if it says so.
fn run_stage_one<'a>(
    source: &dyn Source,
    active: &ActiveSet<'a>,
    old: Option<Lock>,
    flags: &LockFlags,
    root: &Path,
    keep: Keep,
) -> Result<Settled<'a>, Halt> {
    let plan = plan_lock(active, old.as_ref(), flags).map_err(|r| Halt {
        outcome: Outcome::Refused,
        diagnostics: vec![miette::Report::new(r)],
    })?;
    let mut failures: Vec<Failure> = Vec::new();
    let mut entries = Vec::new();
    for step in plan.steps {
        match step {
            Step::Reuse(entry) => entries.push(entry),
            Step::Resolve(repo) => match source.resolve(repo) {
                Ok(pin) => entries.push(locked(repo, pin)),
                Err(e) => failures.push(Failure::new(repo.id, e)),
            },
        }
    }
    let verify_opts = VerifyOpts {
        offline: flags.offline,
    };
    let mut passed = Vec::new();
    for entry in entries {
        let repo = active
            .get(&entry.id)
            .expect("a step is made per active Repo");
        match source.verify(repo, &entry.pin, verify_opts) {
            Ok(()) => passed.push(entry),
            Err(e) => failures.push(Failure::new(repo.id, e)),
        }
    }
    let settled = settle(active, old.as_ref(), passed, failures, keep);
    if settled.write_lock
        && let Err(e) = settled.lock.write(&Lock::path(root))
    {
        return Err(Halt::failed(e));
    }
    Ok(settled)
}

/// Why stage 2 produced no Plan.
enum StageTwoError {
    /// The Lock does not cover every active Repo.
    NotLocked(NotLocked),
    /// Reading the project or the checkouts failed; every failure is collected.
    Read(Vec<miette::Report>),
}

impl StageTwoError {
    /// The reports to show. Past stage 1 (and not `--check`) the Lock covers every active
    /// Repo, so `NotLocked` is only met by `--check`, which maps it to out of date first.
    fn into_reports(self) -> Vec<miette::Report> {
        match self {
            StageTwoError::NotLocked(e) => vec![e.into()],
            StageTwoError::Read(errors) => errors,
        }
    }
}

/// Read the project and what is on disk, then plan the checkouts.
fn plan_stage_two<'a>(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    coverage: &Coverage<'a>,
    lock: &Lock,
    force: bool,
) -> Result<Plan<'a>, StageTwoError> {
    let mut errors: Vec<miette::Report> = Vec::new();
    let listing = source.list().unwrap_or_else(|e| {
        errors.push(e.into());
        vec![]
    });
    let checkouts = match Checkouts::observe(&coverage.active, &listing, |name| {
        source.inspect(name).map_err(|e| e.for_repo(name))
    }) {
        Ok(checkouts) => Some(checkouts),
        Err(inspect_errors) => {
            errors.extend(inspect_errors);
            None
        }
    };
    let project = match project::observe(root, config) {
        Ok(project) => Some(project),
        Err(e) => {
            errors.push(e);
            None
        }
    };
    let (Some(project), Some(checkouts)) = (project, checkouts) else {
        return Err(StageTwoError::Read(errors));
    };
    if !errors.is_empty() {
        return Err(StageTwoError::Read(errors));
    }
    plan_checkouts(coverage, lock, &checkouts, &project, force).map_err(StageTwoError::NotLocked)
}

/// `--check`: report what applying the plan would do.
fn check(plan: Plan, lock_drifted: bool) -> StageTwo {
    StageTwo {
        outcome: check_outcome(Some(&plan), lock_drifted),
        diagnostics: plan.refusals.into_iter().map(miette::Report::new).collect(),
        changes: Vec::new(),
    }
}

/// Apply the plan (ADR 0006). A failure does not stop the other Repos. The Agent files are
/// written only if no Repo the block lists failed, each file on its own.
fn apply(
    source: &dyn Source,
    root: &Path,
    references_dir: &str,
    plan: Plan,
    flags: &SyncFlags,
) -> StageTwo {
    let opts = MaterialiseOpts {
        offline: flags.offline,
    };
    let mut diagnostics: Vec<miette::Report> = Vec::new();
    let mut outcome = plan.applied_outcome();
    let mut failed_actions = Vec::new();
    let mut changes = Vec::new();
    for (i, action) in plan.repos.iter().enumerate() {
        match run(source, action, opts) {
            Ok(note) => {
                diagnostics.extend(note.map(miette::Report::new));
                changes.push(Changed::Checkout {
                    id: action.id().into(),
                    how: action.how(),
                });
            }
            Err(e) => {
                diagnostics.push(e);
                outcome = Outcome::Failed;
                failed_actions.push(i);
            }
        }
    }
    let held_back = plan.holds_back_writes(&failed_actions);
    diagnostics.extend(plan.refusals.into_iter().map(miette::Report::new));
    if !held_back {
        for write in plan.writes {
            match agent_file::write(&root.join(&write.path), &write.text) {
                Ok(()) => changes.push(Changed::AgentFile(write.path)),
                Err(e) => {
                    diagnostics.push(e.into());
                    outcome = Outcome::Failed;
                }
            }
        }
    }
    match plan.exclude {
        Some(ExcludeAction::Ensure) => {
            if let Err(e) = project::ensure_exclude(root, references_dir) {
                diagnostics.push(e.into());
                outcome = Outcome::Failed;
            }
        }
        Some(ExcludeAction::NoGit) => diagnostics.push(Note::NoGitRepo.into()),
        None => {}
    }
    StageTwo {
        outcome,
        diagnostics,
        changes,
    }
}

/// Do one `RepoAction`; on success the note it carries, if any.
fn run(
    source: &dyn Source,
    action: &RepoAction,
    opts: MaterialiseOpts,
) -> Result<Option<Note>, miette::Report> {
    match action {
        RepoAction::Materialise { repo, pin, .. } => {
            source
                .materialise(*repo, pin, opts)
                .map_err(|e| e.for_repo(repo.id))?;
            Ok(None)
        }
        RepoAction::Replace {
            repo, pin, note, ..
        } => {
            source.remove(repo.id).map_err(|e| e.for_repo(repo.id))?;
            source
                .materialise(*repo, pin, opts)
                .map_err(|e| e.for_repo(repo.id))?;
            Ok(note.clone())
        }
        RepoAction::Remove { id } => {
            source.remove(id).map_err(|e| e.for_repo(id))?;
            Ok(None)
        }
    }
}
