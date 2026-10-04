//! The `sync` executor (ADR 0006): reads the project, runs the two stages, applies the plan.
//! Decisions live in `plan`; this module only does what the plans say and collects failures.

use std::collections::HashMap;
use std::path::Path;

use crate::active::{ActiveSet, active};
use crate::agent_file;
use crate::config::{self, Config};
use crate::diagnostic::{NotLocked, NotObserved, Note};
use crate::list::Status;
use crate::lock::{Lock, LockedRepo};
use crate::plan::{
    Checkouts, Drift, ExcludeAction, LockFlags, Plan, RepoAction, Step, lock_drift, plan_checkouts,
    plan_lock,
};
use crate::project;
use crate::source::{MaterialiseOpts, Observed, Source, VerifyOpts};

#[derive(Debug, Default)]
pub struct SyncFlags {
    pub offline: bool,
    pub force: bool,
    pub check: bool,
}

/// How a run ended; the CLI maps it to an exit code (0, 3, 1, 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    InSync,
    OutOfDate,
    Refused,
    Failed,
}

#[derive(Debug)]
pub struct Report {
    pub outcome: Outcome,
    /// How the Lock differed from the config when the run began.
    pub drift: Vec<Drift>,
    /// Refusals, failures and notes, in the order they happened.
    pub diagnostics: Vec<miette::Report>,
}

impl Report {
    fn new(outcome: Outcome, drift: Vec<Drift>, diagnostics: Vec<miette::Report>) -> Report {
        Report {
            outcome,
            drift,
            diagnostics,
        }
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
        Ok(_) => Report::new(Outcome::InSync, drift, vec![]),
        Err(report) => report.with_drift(drift),
    }
}

/// `refs sync`. With `check`, nothing is resolved, verified, fetched or written.
pub fn sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    let Preflight { active, old, drift } = match preflight(root, config) {
        Ok(preflight) => preflight,
        Err(report) => return report,
    };
    let lock = if flags.check {
        match old {
            Some(lock) => lock,
            // Nothing to plan against: the Lock is what is out of date.
            None => return Report::new(Outcome::OutOfDate, drift, vec![]),
        }
    } else {
        match stage_one(source, &active, old, &lock_flags(flags), root) {
            Ok(lock) => lock,
            Err(report) => return report.with_drift(drift),
        }
    };
    plan_and_apply(source, root, config, &active, &lock, drift, flags)
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
    active: &ActiveSet,
    lock: &Lock,
    drift: Vec<Drift>,
    flags: &SyncFlags,
) -> Report {
    let plan = match plan_stage_two(source, root, config, active, lock, flags.force) {
        Ok(plan) => plan,
        // Only `--check` can meet a Lock that does not cover the active set, and it does not
        // repair it: the Lock is what is out of date.
        Err(StageTwoError::NotLocked(_)) if flags.check => {
            return Report::new(Outcome::OutOfDate, drift, vec![]);
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

/// What happened to `refs.toml` in `sync_edited`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The edit gave the text it already had; nothing was written.
    Unchanged,
    /// `refs.toml` (and, by stage 1, `refs.lock`) was written.
    Written,
    /// Stage 1 failed against the edit, or the text is invalid: nothing was written.
    Rejected,
}

#[derive(Debug)]
pub struct Edited {
    pub change: Change,
    pub report: Report,
}

fn rejected(report: Report) -> Edited {
    Edited {
        change: Change::Rejected,
        report,
    }
}

/// `refs.toml` was `before` and has been edited to `after`: run stage 1 against that
/// config, and only if it passes write `refs.toml` (stage 1 writes the Lock), then run stage 2
/// on the Lock stage 1 produced, so each Repo is verified once. A stage 2 failure keeps the
/// edit and the Lock; `refs sync` retries it. An edit that changes nothing writes nothing and
/// just syncs. `flags.check` is not for this function: it writes. The Lock is written before
/// `refs.toml`, so a failed write of `refs.toml` leaves a Lock with an entry the config does
/// not have, which the next `sync` removes.
pub fn sync_edited(
    source: &dyn Source,
    root: &Path,
    before: &str,
    after: &str,
    flags: &SyncFlags,
) -> Edited {
    let config = match config::parse(after) {
        Ok(config) => config,
        Err(e) => {
            return rejected(Report::failed(e));
        }
    };
    if after == before {
        return Edited {
            change: Change::Unchanged,
            report: sync(source, root, &config, flags),
        };
    }
    let Preflight { active, old, drift } = match preflight(root, &config) {
        Ok(preflight) => preflight,
        Err(report) => {
            return rejected(report);
        }
    };
    let lock = match stage_one(source, &active, old, &lock_flags(flags), root) {
        Ok(lock) => lock,
        Err(report) => {
            return rejected(report.with_drift(drift));
        }
    };
    if let Err(e) = project::write_config(root, after) {
        return rejected(Report::failed(e));
    }
    Edited {
        change: Change::Written,
        report: plan_and_apply(source, root, &config, &active, &lock, drift, flags),
    }
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

/// Resolve or reuse every Repo, verify them all, and write the Lock if it changed. Every
/// error is collected; the Lock is written only when there are none.
fn stage_one(
    source: &dyn Source,
    active: &ActiveSet,
    old: Option<Lock>,
    flags: &LockFlags,
    root: &Path,
) -> Result<Lock, Report> {
    let plan = plan_lock(active, old.as_ref(), flags)
        .map_err(|r| Report::new(Outcome::Refused, vec![], vec![miette::Report::new(r)]))?;
    let mut errors: Vec<miette::Report> = Vec::new();
    let mut entries = Vec::new();
    for step in plan.steps {
        match step {
            Step::Reuse(entry) => entries.push(entry),
            Step::Resolve(repo) => match source.resolve(repo) {
                Ok(pin) => entries.push(LockedRepo {
                    id: repo.id.into(),
                    pin,
                }),
                Err(e) => errors.push(e.into()),
            },
        }
    }
    let verify_opts = VerifyOpts {
        offline: flags.offline,
    };
    for entry in &entries {
        let repo = active
            .get(&entry.id)
            .expect("a step is made per active Repo");
        if let Err(e) = source.verify(repo, &entry.pin, verify_opts) {
            errors.push(e.into());
        }
    }
    if !errors.is_empty() {
        return Err(Report::new(Outcome::Failed, vec![], errors));
    }
    let lock = Lock::new(entries);
    if old.as_ref() != Some(&lock)
        && let Err(e) = lock.write(&Lock::path(root))
    {
        return Err(Report::failed(e));
    }
    Ok(lock)
}

/// Why stage 2 produced no Plan.
enum StageTwoError {
    /// The Lock does not cover every active Repo.
    NotLocked(NotLocked),
    /// A bug: `plan_stage_two` inspects every active Repo.
    NotObserved(NotObserved),
    /// Reading the project or the checkouts failed; every failure is collected.
    Read(Vec<miette::Report>),
}

impl StageTwoError {
    /// The reports to show. Past stage 1 (and not `--check`) the Lock covers every active
    /// Repo, so `NotLocked` is then a bug, like `NotObserved`.
    fn into_reports(self) -> Vec<miette::Report> {
        match self {
            StageTwoError::NotLocked(e) => vec![e.into()],
            StageTwoError::NotObserved(e) => vec![e.into()],
            StageTwoError::Read(errors) => errors,
        }
    }
}

/// Read the project and what is on disk, then plan the checkouts.
fn plan_stage_two<'a>(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    active: &ActiveSet<'a>,
    lock: &Lock,
    force: bool,
) -> Result<Plan<'a>, StageTwoError> {
    let mut errors: Vec<miette::Report> = Vec::new();
    let listing = source.list().unwrap_or_else(|e| {
        errors.push(e.into());
        vec![]
    });
    let mut observed: Vec<(String, Observed)> = Vec::new();
    let names = active
        .repos()
        .map(|r| r.id.to_string())
        .chain(listing.iter().filter(|n| active.get(n).is_none()).cloned());
    for name in names {
        match source.inspect(&name) {
            Ok(o) => {
                observed.push((name, o));
            }
            Err(e) => errors.push(e.into()),
        }
    }
    let project = match project::observe(root, config) {
        Ok(project) => Some(project),
        Err(e) => {
            errors.push(e);
            None
        }
    };
    let Some(project) = project.filter(|_| errors.is_empty()) else {
        return Err(StageTwoError::Read(errors));
    };
    let checkouts = Checkouts::new(active, observed).map_err(StageTwoError::NotObserved)?;
    plan_checkouts(active, lock, &checkouts, &project, force).map_err(StageTwoError::NotLocked)
}

/// `--check`: report what applying the plan would do.
fn check(plan: Plan, drift: Vec<Drift>) -> Report {
    let refused = !plan.refusals.is_empty();
    let out_of_date = plan.is_drift() || !drift.is_empty();
    let outcome = match () {
        _ if refused => Outcome::Refused,
        _ if out_of_date => Outcome::OutOfDate,
        _ => Outcome::InSync,
    };
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
    let mut listed_failed = false;
    for action in plan.repos {
        let gates = action.gates_writes();
        let note = match run(source, action, opts) {
            Ok(note) => note,
            Err(e) => {
                diagnostics.push(e);
                failed = true;
                listed_failed |= gates;
                continue;
            }
        };
        diagnostics.extend(note.map(miette::Report::new));
    }
    let refused = !plan.refusals.is_empty();
    diagnostics.extend(plan.refusals.into_iter().map(miette::Report::new));
    if !listed_failed {
        for write in plan.writes {
            if let Err(e) = agent_file::write(&root.join(&write.path), &write.text) {
                diagnostics.push(e.into());
                failed = true;
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
    let outcome = match () {
        _ if failed => Outcome::Failed,
        _ if refused => Outcome::Refused,
        _ => Outcome::InSync,
    };
    Report::new(outcome, drift, diagnostics)
}

/// Do one `RepoAction`; on success the note it carries, if any.
fn run(
    source: &dyn Source,
    action: RepoAction,
    opts: MaterialiseOpts,
) -> Result<Option<Note>, miette::Report> {
    match action {
        RepoAction::Materialise { repo, pin } => {
            source.materialise(repo, &pin, opts)?;
            Ok(None)
        }
        RepoAction::Replace { repo, pin, note } => {
            source.remove(repo.id)?;
            source.materialise(repo, &pin, opts)?;
            Ok(note)
        }
        RepoAction::Remove { id } => {
            source.remove(&id)?;
            Ok(None)
        }
    }
}
