//! The `sync` executor (ADR 0006): reads the project, runs the typed actions of the two
//! plans through `Source`, and collects what happened. Which names are observed, how Lock
//! entries are built, and how a Plan becomes an outcome are decided in `plan`.

use std::path::Path;

use crate::active::{ActiveSet, active};
use crate::agent_file;
use crate::config::Config;
use crate::diagnostic::Note;
use crate::edit::Edit;
use crate::exclude;
use crate::lock::{Lock, LockedRepo};
pub use crate::plan::{Checkout, Hint, Outcome};
use crate::plan::{
    Checkouts, Drift, Exclude, Plan, RepoAction, Step, check_outcome, lock_drift, plan_checkouts,
    plan_lock, shrunk_lock,
};
use crate::project;
use crate::source::Source;
use crate::tooling;

#[derive(Debug, Default)]
pub struct SyncFlags {
    pub offline: bool,
    pub force: bool,
    pub check: bool,
    /// Floating Repos to re-resolve in the Lock. `update` sets it; `sync` has no such flag, but
    /// reads it all the same (through `resolve_all`).
    pub reresolve: Vec<String>,
}

/// Something a run changed on disk, for the CLI to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// `refs.toml` was written (only an edit does).
    Config,
    /// `refs.toml` did not exist, and the edit created it.
    ConfigCreated,
    /// The edit created this Group in `refs.toml`.
    Group(String),
    /// `refs.lock` was written.
    Lock,
    /// The Checkout of this Repo was created, moved or removed.
    Checkout { id: String, how: Checkout },
    /// This Agent file was written (path relative to the Project).
    AgentFile(String),
}

/// How a run went, for the CLI to print: the one result of `sync` and every edit.
#[derive(Debug)]
pub struct Report {
    pub outcome: Outcome,
    /// What the run changed on disk, in the order the CLI says it.
    pub changes: Vec<Changed>,
    /// How the Lock differed from the config when the run began.
    pub drift: Vec<Drift>,
    /// Refusals, failures and notes, in the order they happened.
    pub diagnostics: Vec<miette::Report>,
    /// What to tell the user to do next, if anything. Set where `sync` knows why.
    pub hint: Option<Hint>,
}

impl Report {
    fn new(outcome: Outcome) -> Report {
        Report {
            outcome,
            changes: Vec::new(),
            drift: Vec::new(),
            diagnostics: Vec::new(),
            hint: None,
        }
    }

    /// The editor hint (spec 7.6), when `first` and the run got through.
    fn with_editor_hint(mut self, first: bool, config: &Config) -> Report {
        if first && self.outcome == Outcome::InSync {
            let references_dir = config.settings.references_dir().to_owned();
            self.diagnostics
                .push(miette::Report::new(Note::EditorHint { references_dir }));
        }
        self
    }

    fn of(outcome: Outcome, diagnostics: Vec<miette::Report>) -> Report {
        Report {
            diagnostics,
            ..Report::new(outcome)
        }
    }

    fn failed(error: impl miette::Diagnostic + Send + Sync + 'static) -> Report {
        Report::of(Outcome::Failed, vec![miette::Report::new(error)])
    }

    fn with_drift(mut self, drift: Vec<Drift>) -> Report {
        self.drift = drift;
        self
    }

    fn with_hint(mut self, hint: Hint) -> Report {
        self.hint = Some(hint);
        self
    }

    /// Give `hint` if the run failed or was refused, so left the project incomplete.
    fn hint_if_incomplete(self, hint: Hint) -> Report {
        match self.outcome {
            Outcome::Failed | Outcome::Refused => self.with_hint(hint),
            Outcome::InSync | Outcome::OutOfDate => self,
        }
    }
}

/// Stage 1 as it left things: the Lock stage 2 plans against (with `--check`, the one on
/// disk) and whether it was written.
struct StageOne {
    lock: Lock,
    wrote_lock: bool,
}

/// The Report of a run that got through stage 1: `stage_two` is what stage 2 found or did,
/// and the Lock write came before everything stage 2 did.
fn finish(stage_one: StageOne, drift: Vec<Drift>, stage_two: Report) -> Report {
    with_lock_change(stage_one, stage_two).with_drift(drift)
}

/// `stage_two`, with the Lock write that came before everything in it said first.
fn with_lock_change(stage_one: StageOne, mut stage_two: Report) -> Report {
    if stage_one.wrote_lock {
        stage_two.changes.insert(0, Changed::Lock);
    }
    stage_two
}

/// What every run starts from: the active set, the Lock on disk (if any) and how it differs.
struct Observation<'a> {
    active: ActiveSet<'a>,
    old: Option<Lock>,
    drift: Vec<Drift>,
}

fn observe<'a>(root: &Path, config: &'a Config) -> Result<Observation<'a>, Report> {
    let active = active(config);
    let old = Lock::read(&Lock::path(root)).map_err(Report::failed)?;
    let drift = lock_drift(&active, old.as_ref());
    Ok(Observation { active, old, drift })
}

/// `refs sync`. With `check`, nothing is resolved, verified, fetched or written.
pub fn sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    run_sync(source, root, config, flags).hint_if_incomplete(Hint::RunSyncOnceFixed)
}

fn run_sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    let Observation { active, old, drift } = match observe(root, config) {
        Ok(observation) => observation,
        Err(report) => return report,
    };
    let stage_one = if flags.check {
        // With no Lock on disk the drift says so, and `plan_and_apply` reports it.
        StageOne {
            lock: old.unwrap_or_else(|| Lock::new(vec![])),
            wrote_lock: false,
        }
    } else {
        match stage_one(source, &active, old, flags, root) {
            Ok(stage_one) => stage_one,
            Err(report) => return report.with_drift(drift),
        }
    };
    plan_and_apply(source, root, config, &active, stage_one, drift, flags)
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
        let stage_two = Report::new(check_outcome(None, true));
        return finish(stage_one, drift, stage_two);
    }
    let plan = match plan_stage_two(source, root, config, active, &stage_one.lock, flags.force) {
        Ok(plan) => plan,
        Err(errors) => {
            let stage_two = Report::of(Outcome::Failed, errors);
            return finish(stage_one, drift, stage_two);
        }
    };
    let stage_two = if flags.check {
        check(plan, !drift.is_empty())
    } else {
        apply_plan(source, root, config, active, plan, flags)
    };
    finish(stage_one, drift, stage_two)
}

/// Apply the stage 2 plan and, for a run that got through, add the notes about tooling.
fn apply_plan(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    active: &ActiveSet,
    plan: Plan,
    flags: &SyncFlags,
) -> Report {
    let dir = config.settings.references_dir();
    let mut report = apply(source, root, dir, plan, flags);
    // Only a run that got through, as `-q` hides notes only then (`print_report`); with no
    // active Repo there is no references directory to keep out.
    if report.outcome == Outcome::InSync && active.repos().next().is_some() {
        let gaps = tooling::gaps(root, dir, &config.tooling_ignored());
        report.diagnostics.extend(
            gaps.into_iter()
                .map(|gap| Note::ToolingGap { gap })
                .map(miette::Report::new),
        );
    }
    report
}

/// A failed edit: `refs.toml` is as it was, and `--no-sync` would not help.
fn rejected(error: impl miette::Diagnostic + Send + Sync + 'static) -> Report {
    Report::failed(error).with_hint(Hint::ConfigUnchanged)
}

/// An edit applied to the text of `<root>/refs.toml` and parsed, but not yet written: the
/// config a sync runs against.
struct Applied {
    /// Whether the edit starts `refs.toml` from nothing.
    created: bool,
    before: String,
    after: String,
    config: Config,
    group_created: Option<String>,
    /// Whether the edit is an `add` that takes the Project from no active Repo to one.
    first_active: bool,
}

impl Applied {
    fn read(root: &Path, edit: &Edit) -> Result<Applied, Report> {
        let created = !root.join(project::CONFIG_FILE).exists();
        let before = match created {
            true => String::new(),
            false => project::read_config(root).map_err(rejected)?,
        };
        let applied = edit.apply(&before).map_err(rejected)?;
        Ok(Applied {
            first_active: applied.first_active,
            created,
            before,
            after: applied.text,
            config: applied.config,
            group_created: applied.group_created,
        })
    }

    fn unchanged(&self) -> bool {
        self.after == self.before
    }

    /// `report` of a run that came after the edit was written: what the edit did is said
    /// first.
    fn written(self, mut report: Report) -> Report {
        if self.unchanged() {
            return report;
        }
        let mut edit = vec![if self.created {
            Changed::ConfigCreated
        } else {
            Changed::Config
        }];
        edit.extend(self.group_created.map(Changed::Group));
        report.changes.splice(0..0, edit);
        report
    }
}

/// Apply `edit` to `<root>/refs.toml` and write it, without syncing (`--no-sync`): no Source
/// is needed, and nothing is resolved or verified. An edit that changes nothing writes
/// nothing.
pub fn edit_only(root: &Path, edit: &Edit) -> Report {
    let applied = match Applied::read(root, edit) {
        Ok(applied) => applied,
        Err(report) => return report,
    };
    if applied.unchanged() {
        return Report::new(Outcome::InSync);
    }
    match project::write_config(root, &applied.after) {
        Ok(()) => {
            let report = Report::new(Outcome::InSync)
                .with_editor_hint(applied.first_active, &applied.config);
            applied.written(report).with_hint(Hint::RunSync)
        }
        Err(e) => rejected(e),
    }
}

/// Apply `edit` to `<root>/refs.toml` and sync the project to it.
///
/// Stage 1 runs against the edited config, and `refs.toml` is written only if it passes
/// (stage 1 writes the Lock), then stage 2 runs on the Lock stage 1 produced, so each Repo
/// is verified once. A Repo that cannot be locked blocks the edit: nothing is written and the
/// error names it. A stage 2 refusal blocks it the same way (`plan_stage_two` is planned on
/// the Lock before anything is written). The one exception is an edit that only shrinks the active set (`remove`,
/// `disable`) while the Lock covers every remaining Repo: the Lock is pruned, with nothing
/// resolved or verified, so a Repo the edit did not touch cannot block it. A stage 2 failure
/// keeps the edit and the Lock; `refs sync` retries it.
/// The report says the write of `refs.toml` first (`Changed::Config`, then any `Group`), and
/// has a `Hint` for an edit that was not written: a write that fails says only that
/// `refs.toml` is as it was. An edit that changes nothing goes the same way and writes no
/// `refs.toml`, so `flags.reresolve` can move a Lock entry alone and a refusal still blocks it;
/// its failures are hinted as a failed `sync` is. `flags.check` is not for this function: it
/// writes. The Lock is written before `refs.toml`, so a failed write of `refs.toml` leaves a
/// Lock with an entry the config does not have, which the next `sync` removes.
pub fn edit(source: &dyn Source, root: &Path, edit: &Edit, flags: &SyncFlags) -> Report {
    let applied = match Applied::read(root, edit) {
        Ok(applied) => applied,
        Err(report) => return report,
    };
    let unchanged = applied.unchanged();
    let report = apply_edit(source, root, applied, flags);
    if unchanged {
        // "refs.toml is as it was" says nothing about an edit that had nothing to write.
        report.hint_if_incomplete(Hint::RunSyncOnceFixed)
    } else {
        report
    }
}

fn apply_edit(source: &dyn Source, root: &Path, applied: Applied, flags: &SyncFlags) -> Report {
    let Observation { active, old, drift } = match observe(root, &applied.config) {
        Ok(observation) => observation,
        Err(report) => return report.with_hint(Hint::ConfigUnchanged),
    };
    let report = match edit_stages(source, root, &applied, &active, old, flags) {
        Ok(report) => applied.written(report),
        Err(report) => report,
    };
    report.with_drift(drift)
}

/// Both stages of an edit. `Ok` is a run that wrote `refs.toml` (what stage 2 found or did);
/// `Err` is one that stopped before, or failed to write it.
fn edit_stages(
    source: &dyn Source,
    root: &Path,
    applied: &Applied,
    active: &ActiveSet,
    old: Option<Lock>,
    flags: &SyncFlags,
) -> Result<Report, Report> {
    let config = &applied.config;
    let lock = match shrunk_lock(active, old.as_ref()) {
        Some(lock) => lock,
        None => resolve_all(source, active, old.as_ref(), flags)
            .map_err(|report| report.with_hint(Hint::ConfigUnchangedTryNoSync))?,
    };
    let plan = stage_two_gate(source, root, config, active, &lock, flags.force)?;
    let stage_one = commit(root, old.as_ref(), lock)
        .map_err(|report| report.with_hint(Hint::ConfigUnchanged))?;
    if !applied.unchanged()
        && let Err(e) = project::write_config(root, &applied.after)
    {
        return Err(with_lock_change(stage_one, rejected(e)));
    }
    let stage_two = apply_plan(source, root, config, active, plan, flags)
        .with_editor_hint(applied.first_active, config)
        .hint_if_incomplete(Hint::FixOrRemoveThenSync);
    Ok(with_lock_change(stage_one, stage_two))
}

/// The stage 2 plan an edit may apply. Stage 2's refusals are predictable, so they are found
/// before anything is written: an edit that stage 2 would refuse changes nothing, and a retry
/// refuses the same way. What cannot be observed is treated alike, since then nothing says
/// stage 2 would not refuse.
fn stage_two_gate<'a>(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    active: &ActiveSet<'a>,
    lock: &Lock,
    force: bool,
) -> Result<Plan<'a>, Report> {
    match plan_stage_two(source, root, config, active, lock, force) {
        Ok(plan) if plan.refusals.is_empty() => Ok(plan),
        Ok(plan) => Err(Report::of(
            Outcome::Refused,
            plan.refusals.into_iter().map(miette::Report::new).collect(),
        )
        .with_hint(Hint::ConfigUnchanged)),
        Err(errors) => {
            Err(Report::of(Outcome::Failed, errors).with_hint(Hint::ConfigUnchangedUnobserved))
        }
    }
}

/// Stage 1 as `sync` and an edit run it: resolve and verify, then write the Lock.
fn stage_one(
    source: &dyn Source,
    active: &ActiveSet,
    old: Option<Lock>,
    flags: &SyncFlags,
    root: &Path,
) -> Result<StageOne, Report> {
    let lock = resolve_all(source, active, old.as_ref(), flags)?;
    commit(root, old.as_ref(), lock)
}

/// Resolve or reuse every Repo and verify them all, collecting every error. All or nothing:
/// a failure of any Repo fails the run with every error.
fn resolve_all(
    source: &dyn Source,
    active: &ActiveSet,
    old: Option<&Lock>,
    flags: &SyncFlags,
) -> Result<Lock, Report> {
    let steps = plan_lock(active, old, &flags.reresolve, flags.offline)
        .map_err(|refusal| Report::of(Outcome::Refused, vec![refusal.into()]))?;
    let mut errors: Vec<miette::Report> = Vec::new();
    let mut entries = Vec::new();
    for step in steps {
        match step {
            Step::Reuse(entry) => entries.push(entry),
            Step::Resolve(repo) => match source.resolve(repo) {
                Ok(pin) => entries.push(LockedRepo {
                    id: repo.id.into(),
                    pin,
                }),
                Err(e) => errors.push(e.for_repo(repo.id)),
            },
        }
    }
    let mut passed = Vec::new();
    for entry in entries {
        let repo = active
            .get(&entry.id)
            .expect("a step is made per active Repo");
        match source.verify(repo, &entry.pin, flags.offline) {
            Ok(()) => passed.push(entry),
            Err(e) => errors.push(e.for_repo(repo.id)),
        }
    }
    if errors.is_empty() {
        Ok(Lock::new(passed))
    } else {
        Err(Report::of(Outcome::Failed, errors))
    }
}

/// Write `lock` if it differs from the one on disk.
fn commit(root: &Path, old: Option<&Lock>, lock: Lock) -> Result<StageOne, Report> {
    let wrote_lock = old != Some(&lock);
    if wrote_lock && let Err(e) = lock.write(&Lock::path(root)) {
        return Err(Report::failed(e));
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
fn check(plan: Plan, lock_drifted: bool) -> Report {
    let outcome = check_outcome(Some(&plan), lock_drifted);
    Report::of(
        outcome,
        plan.refusals.into_iter().map(miette::Report::new).collect(),
    )
}

/// Apply the plan (ADR 0006). A failure does not stop the other Repos. The Agent files are
/// written only if no Repo the block lists failed, each file on its own.
fn apply(
    source: &dyn Source,
    root: &Path,
    references_dir: &str,
    plan: Plan,
    flags: &SyncFlags,
) -> Report {
    let mut diagnostics: Vec<miette::Report> = Vec::new();
    let mut outcome = plan.applied_outcome();
    let mut failed_actions = Vec::new();
    let mut changes = Vec::new();
    for (i, action) in plan.repos.iter().enumerate() {
        match run(source, action, flags.offline) {
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
        Exclude::Missing => {
            if let Err(e) = exclude::ensure(root, references_dir) {
                diagnostics.push(e.into());
                outcome = Outcome::Failed;
            }
        }
        Exclude::NoGit => diagnostics.push(Note::NoGitRepo.into()),
        Exclude::Present => {}
    }
    Report {
        changes,
        diagnostics,
        ..Report::new(outcome)
    }
}

/// Do one `RepoAction`; on success the note it carries, if any.
fn run(
    source: &dyn Source,
    action: &RepoAction,
    offline: bool,
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
                .materialise(*repo, pin, offline)
                .map_err(|e| e.for_repo(repo.id))?;
            Ok(action.note())
        }
        RepoAction::Remove { id } => {
            source.remove(id).map_err(|e| e.for_repo(id))?;
            Ok(None)
        }
    }
}
