//! The `sync` executor (ADR 0006): reads the project, runs the typed actions of the two
//! plans through `Source`, and collects what happened. Which names are observed, how Lock
//! entries are built, and how a Plan becomes an outcome are decided in `plan`.

use std::path::Path;

use crate::active::{ActiveSet, active};
use crate::agent_file;
use crate::config::{self, Config};
use crate::diagnostic::Note;
use crate::edit::Edit;
use crate::exclude;
use crate::lock::Lock;
pub use crate::plan::{Change, Checkout, Hint, Outcome};
use crate::plan::{
    Checkouts, Command, Drift, ExcludeAction, LockFlags, Plan, RepoAction, Step, check_outcome,
    hint, lock_drift, locked, plan_checkouts, plan_lock, shrunk_lock,
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
    /// What to tell the user to do next, if anything.
    pub hint: Option<Hint>,
}

impl Report {
    fn new(outcome: Outcome, drift: Vec<Drift>, diagnostics: Vec<miette::Report>) -> Report {
        Report {
            outcome,
            changes: Vec::new(),
            drift,
            diagnostics,
            hint: None,
        }
    }

    fn with_hint(mut self, command: Command) -> Report {
        self.hint = hint(command, self.outcome);
        self
    }

    fn failed(error: impl miette::Diagnostic + Send + Sync + 'static) -> Report {
        Report::new(Outcome::Failed, vec![], vec![miette::Report::new(error)])
    }
}

/// Stage 1 as it left things: the Lock stage 2 plans against (with `--check`, the one on
/// disk) and whether it was written.
struct StageOne {
    lock: Lock,
    wrote_lock: bool,
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

/// The Report of a run that got through stage 1: the outcome is stage 2's, and the Lock write
/// came before everything stage 2 did.
fn finish(stage_one: StageOne, drift: Vec<Drift>, stage_two: StageTwo) -> Report {
    let mut changes = stage_two.changes;
    if stage_one.wrote_lock {
        changes.insert(0, Changed::Lock);
    }
    Report {
        outcome: stage_two.outcome,
        changes,
        drift,
        diagnostics: stage_two.diagnostics,
        hint: None,
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
        Ok(stage_one) => finish(stage_one, drift, StageTwo::only(Outcome::InSync)),
        Err(halt) => halt.into_report(drift),
    }
}

/// `refs sync`. With `check`, nothing is resolved, verified, fetched or written.
pub fn sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    run_sync(source, root, config, flags).with_hint(Command::Sync)
}

fn run_sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    let Preflight { active, old, drift } = match preflight(root, config) {
        Ok(preflight) => preflight,
        Err(report) => return report,
    };
    let stage_one = if flags.check {
        match old {
            Some(lock) => StageOne {
                lock,
                wrote_lock: false,
            },
            // Nothing to plan against: the Lock is what is out of date.
            None => {
                let stage_two = StageTwo::only(check_outcome(None, true));
                let stage_one = StageOne {
                    lock: Lock::new(vec![]),
                    wrote_lock: false,
                };
                return finish(stage_one, drift, stage_two);
            }
        }
    } else {
        match stage_one(source, &active, old, &lock_flags(flags), root) {
            Ok(stage_one) => stage_one,
            Err(halt) => return halt.into_report(drift),
        }
    };
    plan_and_apply(source, root, config, &active, stage_one, drift, flags)
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
    stage_one: StageOne,
    drift: Vec<Drift>,
    flags: &SyncFlags,
) -> Report {
    // Only `--check` can meet a Lock that does not cover the active set, and it does not
    // repair it: the Lock is what is out of date, and there is nothing to plan against.
    let uncovered = drift
        .iter()
        .any(|d| matches!(d, Drift::LockMissing | Drift::Added(_)));
    if flags.check && uncovered {
        let stage_two = StageTwo::only(check_outcome(None, true));
        return finish(stage_one, drift, stage_two);
    }
    let plan = match plan_stage_two(source, root, config, active, &stage_one.lock, flags.force) {
        Ok(plan) => plan,
        Err(errors) => {
            let stage_two = StageTwo {
                diagnostics: errors,
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
    unwritten(Change::Rejected, report)
}

fn rejected_by(error: impl miette::Diagnostic + Send + Sync + 'static) -> Edited {
    rejected(Report::failed(error))
}

/// The edit was valid but stage 1 failed against it.
fn blocked(report: Report) -> Edited {
    unwritten(Change::Blocked, report)
}

fn unwritten(change: Change, report: Report) -> Edited {
    // An edit that was not written never syncs, so `no_sync` does not change its hint.
    let command = Command::Edit {
        change,
        no_sync: false,
    };
    Edited {
        change,
        group_created: None,
        report: report.with_hint(command),
    }
}

/// An edit applied to the text of `<root>/refs.toml` and parsed, but not yet written. The
/// edited text is parsed once, here, into the config a sync runs against.
struct Applied {
    before: String,
    after: String,
    config: Config,
    group_created: Option<String>,
}

impl Applied {
    fn read(root: &Path, edit: &Edit) -> Result<Applied, Edited> {
        let before = project::read_config(root).map_err(rejected_by)?;
        let applied = edit.apply(&before).map_err(rejected_by)?;
        let config = config::parse(&applied.text).map_err(rejected_by)?;
        Ok(Applied {
            before,
            after: applied.text,
            config,
            group_created: applied.group_created,
        })
    }

    fn unchanged(&self) -> bool {
        self.after == self.before
    }

    fn edited(&self, change: Change, report: Report, no_sync: bool) -> Edited {
        Edited {
            change,
            group_created: self.group_created.clone(),
            report: report.with_hint(Command::Edit { change, no_sync }),
        }
    }
}

/// Apply `edit` to `<root>/refs.toml` and write it, without syncing (`--no-sync`): no Source
/// is needed, and nothing is resolved or verified. An edit that changes nothing writes
/// nothing.
pub fn edit_only(root: &Path, edit: &Edit) -> Edited {
    let applied = match Applied::read(root, edit) {
        Ok(applied) => applied,
        Err(edited) => return edited,
    };
    let change = if applied.unchanged() {
        Change::Unchanged
    } else {
        match project::write_config(root, &applied.after) {
            Ok(()) => Change::Written,
            Err(e) => return rejected_by(e),
        }
    };
    applied.edited(change, Report::new(Outcome::InSync, vec![], vec![]), true)
}

/// Apply `edit` to `<root>/refs.toml` and sync the project to it.
///
/// Stage 1 runs against the edited config, and `refs.toml` is written only if it passes
/// (stage 1 writes the Lock), then stage 2 runs on the Lock stage 1 produced, so each Repo
/// is verified once. A Repo that cannot be locked blocks the edit: nothing is written and the
/// error names it. The one exception is an edit that only shrinks the active set (`remove`,
/// `disable`) while the Lock covers every remaining Repo: the Lock is pruned, with nothing
/// resolved or verified, so a Repo the edit did not touch cannot block it. A stage 2 failure
/// keeps the edit and the Lock; `refs sync` retries it.
/// An edit that changes nothing writes nothing and just syncs. `flags.check` is not for
/// this function: it writes. The Lock is written before `refs.toml`, so a failed write of
/// `refs.toml` leaves a Lock with an entry the config does not have, which the next `sync`
/// removes.
pub fn edit(source: &dyn Source, root: &Path, edit: &Edit, flags: &SyncFlags) -> Edited {
    let applied = match Applied::read(root, edit) {
        Ok(applied) => applied,
        Err(edited) => return edited,
    };
    let config = &applied.config;
    if applied.unchanged() {
        let report = sync(source, root, config, flags);
        return applied.edited(Change::Unchanged, report, false);
    }
    let Preflight { active, old, drift } = match preflight(root, config) {
        Ok(preflight) => preflight,
        Err(report) => return rejected(report),
    };
    let stage_one = match shrunk_lock(&active, old.as_ref()) {
        Some(lock) => match lock.write(&Lock::path(root)) {
            Ok(()) => StageOne {
                lock,
                wrote_lock: true,
            },
            Err(e) => return blocked(Report::failed(e)),
        },
        None => match stage_one(source, &active, old, &lock_flags(flags), root) {
            Ok(stage_one) => stage_one,
            Err(halt) => return blocked(halt.into_report(drift)),
        },
    };
    if let Err(e) = project::write_config(root, &applied.after) {
        return rejected(Report::failed(e));
    }
    let report = plan_and_apply(source, root, config, &active, stage_one, drift, flags);
    applied.edited(Change::Written, report, false)
}

/// Stage 1: resolve or reuse every Repo and verify them all, collecting every error. All or
/// nothing: a failure of any Repo fails the run with every error and writes nothing.
fn stage_one(
    source: &dyn Source,
    active: &ActiveSet,
    old: Option<Lock>,
    flags: &LockFlags,
    root: &Path,
) -> Result<StageOne, Halt> {
    let plan = plan_lock(active, old.as_ref(), flags).map_err(|r| Halt {
        outcome: Outcome::Refused,
        diagnostics: vec![miette::Report::new(r)],
    })?;
    let mut errors: Vec<miette::Report> = Vec::new();
    let mut entries = Vec::new();
    for step in plan.steps {
        match step {
            Step::Reuse(entry) => entries.push(entry),
            Step::Resolve(repo) => match source.resolve(repo) {
                Ok(pin) => entries.push(locked(repo, pin)),
                Err(e) => errors.push(e.for_repo(repo.id)),
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
            Err(e) => errors.push(e.for_repo(repo.id)),
        }
    }
    if !errors.is_empty() {
        return Err(Halt {
            outcome: Outcome::Failed,
            diagnostics: errors,
        });
    }
    let lock = Lock::new(passed);
    let wrote_lock = old.as_ref() != Some(&lock);
    if wrote_lock && let Err(e) = lock.write(&Lock::path(root)) {
        return Err(Halt::failed(e));
    }
    Ok(StageOne { lock, wrote_lock })
}

/// Read the project and what is on disk, then plan the checkouts. Reading can fail in
/// several places at once, and every failure is returned. The Lock must cover every active
/// Repo.
fn plan_stage_two<'a>(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    active: &ActiveSet<'a>,
    lock: &Lock,
    force: bool,
) -> Result<Plan<'a>, Vec<miette::Report>> {
    let mut errors: Vec<miette::Report> = Vec::new();
    let listing = source.list().unwrap_or_else(|e| {
        errors.push(e.into());
        vec![]
    });
    let checkouts = match Checkouts::observe(active, &listing, |name| {
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
        return Err(errors);
    };
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(plan_checkouts(active, lock, &checkouts, &project, force))
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
            if let Err(e) = exclude::ensure(root, references_dir) {
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
        RepoAction::Materialise {
            repo,
            pin,
            placement,
        } => {
            if placement.removes_first() {
                source.remove(repo.id).map_err(|e| e.for_repo(repo.id))?;
            }
            source
                .materialise(*repo, pin, opts)
                .map_err(|e| e.for_repo(repo.id))?;
            Ok(action.note())
        }
        RepoAction::Remove { id } => {
            source.remove(id).map_err(|e| e.for_repo(id))?;
            Ok(None)
        }
    }
}
