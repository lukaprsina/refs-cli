//! The `sync` executor (ADR 0006): reads the project, runs the typed actions of the two
//! plans through `Source`, and collects what happened. Which names are observed, how Lock
//! entries are built, and how a Plan becomes an outcome are decided in `plan`.

use std::collections::HashMap;
use std::path::Path;

use crate::active::{ActiveSet, active};
use crate::agent_file;
use crate::config::{self, Config};
use crate::diagnostic::{NotLocked, Note, SourceError};
use crate::edit::Edit;
use crate::list::Status;
use crate::lock::Lock;
pub use crate::plan::{Checkout, Outcome};
use crate::plan::{
    Checkouts, Coverage, Drift, ExcludeAction, Failure, Keep, LockFlags, Plan, RepoAction, Settled,
    Step, check_outcome, lock_drift, locked, plan_checkouts, plan_lock, settle,
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

    fn with_changes(self, changes: Vec<Changed>) -> Report {
        Report { changes, ..self }
    }

    /// Stage 1's Lock write came before everything in this report.
    fn after_lock_write(mut self, wrote: bool) -> Report {
        if wrote {
            self.changes.insert(0, Changed::Lock);
        }
        self
    }

    /// Stage 1 builds its reports without knowing how the Lock differed; the caller does.
    fn with_drift(self, drift: Vec<Drift>) -> Report {
        Report { drift, ..self }
    }

    fn failed(error: impl miette::Diagnostic + Send + Sync + 'static) -> Report {
        Report::new(Outcome::Failed, vec![], vec![miette::Report::new(error)])
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
            Report::new(Outcome::InSync, drift, vec![]).after_lock_write(settled.write_lock)
        }
        Err(report) => report.with_drift(drift),
    }
}

/// `refs sync`. With `check`, nothing is resolved, verified, fetched or written.
pub fn sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    let Preflight { active, old, drift } = match preflight(root, config) {
        Ok(preflight) => preflight,
        Err(report) => return report,
    };
    let (lock, wrote) = if flags.check {
        match old {
            Some(lock) => (lock, false),
            // Nothing to plan against: the Lock is what is out of date.
            None => return Report::new(check_outcome(None, true), drift, vec![]),
        }
    } else {
        match stage_one(source, &active, old, &lock_flags(flags), root) {
            Ok(settled) => (settled.lock, settled.write_lock),
            Err(report) => return report.with_drift(drift),
        }
    };
    let coverage = Coverage::full(active);
    plan_and_apply(source, root, config, &coverage, &lock, drift, flags).after_lock_write(wrote)
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
    lock: &Lock,
    drift: Vec<Drift>,
    flags: &SyncFlags,
) -> Report {
    let plan = match plan_stage_two(source, root, config, coverage, lock, flags.force) {
        Ok(plan) => plan,
        // Only `--check` can meet a Lock that does not cover the active set, and it does not
        // repair it: the Lock is what is out of date.
        Err(StageTwoError::NotLocked(_)) if flags.check => {
            return Report::new(check_outcome(None, true), drift, vec![]);
        }
        Err(e) => return Report::new(Outcome::Failed, drift, e.into_reports()),
    };
    if flags.check {
        check(plan, drift)
    } else {
        let dir = config.settings.references_dir();
        apply(source, root, dir, plan, flags, drift)
    }
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
/// is verified once. A stage 2 failure keeps the edit and the Lock; `refs sync` retries it. When `remove` or `disable` leaves a Repo that cannot be locked, stage 2 still runs for the Repos that did lock (the Checkouts and the block follow the partial Lock) and leaves the failed Repo's Checkout alone.
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
        Err(report) => return rejected(report.with_drift(drift)),
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
    let mut report = plan_and_apply(
        &source,
        root,
        &config,
        &settled.coverage,
        &settled.lock,
        drift,
        flags,
    )
    .after_lock_write(settled.write_lock);
    if !settled.errors.is_empty() {
        report.outcome = Outcome::Failed;
        report.diagnostics.splice(0..0, settled.errors);
    }
    edited(Change::Written, report)
}

/// The Lock and what `inspect` finds for every repo of the config, for `list --status`.
pub fn status(source: &dyn Source, root: &Path, config: &Config) -> Result<Status, miette::Report> {
    let lock = Lock::read(&Lock::path(root)).map_err(miette::Report::new)?;
    let mut observed = HashMap::new();
    for id in config.repos.keys() {
        let id = id.as_ref().as_str();
        let seen = source.inspect(id).map_err(miette::Report::new)?;
        observed.insert(id.to_string(), seen);
    }
    Ok(Status { lock, observed })
}

/// Stage 1 for `lock` and `sync`: all or nothing. A failure of any Repo fails the run with
/// every error and writes nothing.
fn stage_one<'a>(
    source: &dyn Source,
    active: &ActiveSet<'a>,
    old: Option<Lock>,
    flags: &LockFlags,
    root: &Path,
) -> Result<Settled<'a>, Report> {
    let settled = run_stage_one(source, active, old, flags, root, Keep::AllOrNothing)?;
    if settled.accepted {
        Ok(settled)
    } else {
        Err(Report::new(Outcome::Failed, vec![], settled.errors))
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
) -> Result<Settled<'a>, Report> {
    let plan = plan_lock(active, old.as_ref(), flags)
        .map_err(|r| Report::new(Outcome::Refused, vec![], vec![miette::Report::new(r)]))?;
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
        return Err(Report::failed(e));
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
fn check(plan: Plan, drift: Vec<Drift>) -> Report {
    let outcome = check_outcome(Some(&plan), !drift.is_empty());
    let diagnostics = plan.refusals.into_iter().map(miette::Report::new).collect();
    Report::new(outcome, drift, diagnostics)
}

/// Apply the plan (ADR 0006). A failure does not stop the other Repos. The Agent files are
/// written only if no Repo the block lists failed, each file on its own.
fn apply(
    source: &dyn Source,
    root: &Path,
    references_dir: &str,
    plan: Plan,
    flags: &SyncFlags,
    drift: Vec<Drift>,
) -> Report {
    let opts = MaterialiseOpts {
        offline: flags.offline,
    };
    let mut diagnostics: Vec<miette::Report> = Vec::new();
    let mut failed = false;
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
                failed = true;
                failed_actions.push(i);
            }
        }
    }
    let held_back = plan.holds_back_writes(&failed_actions);
    let refused = plan.is_refused();
    diagnostics.extend(plan.refusals.into_iter().map(miette::Report::new));
    if !held_back {
        for write in plan.writes {
            match agent_file::write(&root.join(&write.path), &write.text) {
                Ok(()) => changes.push(Changed::AgentFile(write.path)),
                Err(e) => {
                    diagnostics.push(e.into());
                    failed = true;
                }
            }
        }
    }
    match plan.exclude {
        Some(ExcludeAction::Ensure) => {
            if let Err(e) = project::ensure_exclude(root, references_dir) {
                diagnostics.push(e.into());
                failed = true;
            }
        }
        Some(ExcludeAction::NoGit) => diagnostics.push(Note::NoGitRepo.into()),
        None => {}
    }
    Report::new(Plan::applied_outcome(refused, failed), drift, diagnostics).with_changes(changes)
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
