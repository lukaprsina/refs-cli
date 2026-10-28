//! The `sync` executor (ADR 0006): reads the project, runs the typed actions of the two
//! plans through `Source`, and collects what happened into a `Report`. Which names are
//! observed, how Lock entries are built, and the outcome of `--check` are decided in `plan`;
//! the Hint is set here, from where a run stopped (`hint`).

use std::path::Path;

use crate::active::{ActiveSet, active};
use crate::agent_file;
use crate::config::{self, Config};
use crate::diagnostic::Note;
use crate::edit::{self, Edit};
use crate::exclude;
use crate::lock::{Lock, LockedRepo};
pub use crate::plan::{Checkout, Outcome};
use crate::plan::{
    Checkouts, Drift, Exclude, Plan, RepoAction, Step, lock_drift, plan_checkouts, plan_lock,
    shrunk_lock,
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

impl Changed {
    /// Where it comes in what the CLI says (ADR 0005): `refs.toml`, its Groups, the Lock, the
    /// Checkouts, the Agent files. Not the order of the writes: the Lock is written first.
    fn rank(&self) -> u8 {
        match self {
            Changed::Config | Changed::ConfigCreated => 0,
            Changed::Group(_) => 1,
            Changed::Lock => 2,
            Changed::Checkout { .. } => 3,
            Changed::AgentFile(_) => 4,
        }
    }
}

/// The follow-up the CLI gives a user whose project a run left incomplete or failed, named
/// for why it is given. `hint` picks it from where the run stopped; `cli` only gives each its
/// words (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// `refs.toml` was written without a sync (`--no-sync`): the project is incomplete.
    RunSync,
    /// A `sync` failed or was refused, or an edit that changed nothing did.
    RunSyncOnceFixed,
    /// `refs.toml` was written, then the sync failed or was refused.
    FixOrRemoveThenSync,
    /// The edit was rejected, or a write failed, so `refs.toml` is as it was and `--no-sync`
    /// would not help.
    ConfigUnchanged,
    /// Stage 1 blocked the edit, so `refs.toml` is as it was; `--no-sync` writes it anyway.
    ConfigUnchangedTryNoSync,
    /// Stage 2 could not be observed (the references directory or an Agent file could not
    /// be read), so `refs.toml` is as it was; `--no-sync` writes it anyway.
    ConfigUnchangedUnobserved,
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
    /// What to tell the user to do next, if anything.
    pub hint: Option<Hint>,
}

impl Report {
    fn new(outcome: Outcome, diagnostics: Vec<miette::Report>) -> Report {
        Report {
            outcome,
            changes: Vec::new(),
            drift: Vec::new(),
            diagnostics,
            hint: None,
        }
    }

    fn failed(error: impl miette::Diagnostic + Send + Sync + 'static) -> Report {
        Report::new(Outcome::Failed, vec![miette::Report::new(error)])
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
}

/// Which run it was, for its Hint.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// `refs sync`, or an edit that changes nothing: once it is applied, `refs.toml` is as it
    /// was whatever happens next.
    Sync,
    /// An edit that changes `refs.toml`, then a sync.
    Edit,
    /// An edit written without a sync (`--no-sync`).
    NoSync,
}

/// Where a run that did not get through stage 2 stopped, with what it has to say.
type Stopped = (Stop, Report);

#[derive(Debug, Clone, Copy)]
enum Stop {
    /// The edit could not be read or applied, or `refs.toml` could not be written.
    Rejected,
    /// `refs.lock` could not be read or written.
    Lock,
    /// Stage 1 failed or was refused.
    StageOne,
    /// Stage 2 would refuse, so an edit was not written.
    Refused,
    /// Stage 2 could not be observed, so an edit was not written.
    Unobserved,
}

/// The follow-up for a `kind` of run that stopped at `stop` (`None`: it got through stage 2,
/// to `outcome`).
fn hint(kind: Kind, stop: Option<Stop>, outcome: Outcome) -> Option<Hint> {
    let incomplete = matches!(outcome, Outcome::Failed | Outcome::Refused);
    match (kind, stop) {
        (_, Some(Stop::Rejected)) => Some(Hint::ConfigUnchanged),
        (Kind::Sync, _) => incomplete.then_some(Hint::RunSyncOnceFixed),
        (Kind::Edit, Some(Stop::Lock | Stop::Refused)) => Some(Hint::ConfigUnchanged),
        (Kind::Edit, Some(Stop::StageOne)) => Some(Hint::ConfigUnchangedTryNoSync),
        (Kind::Edit, Some(Stop::Unobserved)) => Some(Hint::ConfigUnchangedUnobserved),
        (Kind::Edit, None) => incomplete.then_some(Hint::FixOrRemoveThenSync),
        (Kind::NoSync, _) => Some(Hint::RunSync),
    }
}

/// The Report of a run: its Hint, and its changes in the order the CLI says them.
fn finish(kind: Kind, run: Result<Report, Stopped>, drift: Vec<Drift>) -> Report {
    let (stop, mut report) = match run {
        Ok(report) => (None, report),
        Err((stop, report)) => (Some(stop), report),
    };
    report.hint = hint(kind, stop, report.outcome);
    report.changes.sort_by_key(Changed::rank);
    report.drift = drift;
    report
}

/// What every run starts from: the active set, the Lock on disk (if any) and how it differs.
struct Observation<'a> {
    active: ActiveSet<'a>,
    old: Option<Lock>,
    drift: Vec<Drift>,
}

fn observe<'a>(root: &Path, config: &'a Config) -> Result<Observation<'a>, Stopped> {
    let active = active(config);
    let old = Lock::read(&Lock::path(root)).map_err(|e| (Stop::Lock, Report::failed(e)))?;
    let drift = lock_drift(&active, old.as_ref());
    Ok(Observation { active, old, drift })
}

/// `refs sync`. With `check`, nothing is resolved, verified, fetched or written.
pub fn sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    let (run, drift) = match observe(root, config) {
        Ok(Observation { active, old, drift }) => {
            let run = run_sync(source, root, config, &active, old, &drift, flags);
            (run, drift)
        }
        Err(stopped) => (Err(stopped), Vec::new()),
    };
    finish(Kind::Sync, run, drift)
}

fn run_sync(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    active: &ActiveSet,
    old: Option<Lock>,
    drift: &[Drift],
    flags: &SyncFlags,
) -> Result<Report, Stopped> {
    let (lock, lock_change) = if flags.check {
        // Only `--check` can meet a Lock that does not cover the active set, and it does not
        // repair it: the Lock is what is out of date, and there is nothing to plan against.
        let uncovered = drift
            .iter()
            .any(|d| matches!(d, Drift::LockMissing | Drift::Added(_)));
        if uncovered {
            return Ok(Report::new(Outcome::OutOfDate, Vec::new()));
        }
        (old.unwrap_or_else(|| Lock::new(vec![])), None)
    } else {
        let lock =
            resolve_all(source, active, old.as_ref(), flags).map_err(|r| (Stop::StageOne, r))?;
        let lock_change = write_lock(root, old.as_ref(), &lock)?;
        (lock, lock_change)
    };
    let mut report = match plan_stage_two(source, root, config, active, &lock, flags.force) {
        Ok(plan) if flags.check => check(plan, !drift.is_empty()),
        Ok(plan) => apply_plan(source, root, config, active, plan, flags),
        Err(errors) => Report::new(Outcome::Failed, errors),
    };
    report.changes.extend(lock_change);
    Ok(report)
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

/// A failed edit, or a failed write of `refs.toml`.
fn rejected(error: impl miette::Diagnostic + Send + Sync + 'static) -> Stopped {
    (Stop::Rejected, Report::failed(error))
}

/// An edit applied to the text of `<root>/refs.toml` and parsed, but not yet written: the
/// config a sync runs against.
struct Edited {
    /// Whether the edit starts `refs.toml` from nothing.
    created: bool,
    before: String,
    applied: edit::Applied,
}

impl Edited {
    fn read(root: &Path, edit: &Edit) -> Result<Edited, Stopped> {
        let created = !root.join(project::CONFIG_FILE).exists();
        let before = match created {
            true => String::new(),
            false => project::read_config(root).map_err(rejected)?,
        };
        let applied = edit.apply(&before).map_err(rejected)?;
        Ok(Edited {
            created,
            before,
            applied,
        })
    }

    /// Whether the edit takes the Project from no active Repo to one. The old text parsed when
    /// it was last written; one a hand edit broke counts as having none.
    fn first_active(&self) -> bool {
        let any = |config: &Config| active(config).repos().next().is_some();
        !config::parse(&self.before).is_ok_and(|old| any(&old)) && any(&self.applied.config)
    }

    fn unchanged(&self) -> bool {
        self.applied.text == self.before
    }

    /// Write `refs.toml` if the edit changes it, and say what was written.
    fn write(&self, root: &Path) -> Result<Vec<Changed>, Stopped> {
        if self.unchanged() {
            return Ok(Vec::new());
        }
        project::write_config(root, &self.applied.text).map_err(rejected)?;
        let config = match self.created {
            true => Changed::ConfigCreated,
            false => Changed::Config,
        };
        let group = self.applied.group_created.clone().map(Changed::Group);
        Ok([config].into_iter().chain(group).collect())
    }
}

/// Apply `edit` to `<root>/refs.toml` and sync the project to it; with no `source`
/// (`--no-sync`), only write it, with nothing resolved or verified. An edit that changes
/// nothing writes nothing.
///
/// Stage 1 runs against the edited config, and `refs.toml` is written only if it passes
/// (stage 1 writes the Lock), then stage 2 runs on the Lock stage 1 produced, so each Repo
/// is verified once. A Repo that cannot be locked blocks the edit: nothing is written and the
/// error names it. A stage 2 refusal blocks it the same way (`stage_two_gate` plans on the
/// Lock before anything is written). The one exception is an edit that only shrinks the
/// active set (`remove`, `disable`) while the Lock covers every remaining Repo: the Lock is
/// pruned, with nothing resolved or verified, so a Repo the edit did not touch cannot block
/// it. A stage 2 failure keeps the edit and the Lock; `refs sync` retries it.
/// An edit that changes nothing still syncs, so `flags.reresolve` can move a Lock entry alone
/// and a refusal still blocks it; it is hinted as a `sync` is. `flags.check` is not for this
/// function: it writes. The Lock is written before `refs.toml`, so a failed write of
/// `refs.toml` leaves a Lock with an entry the config does not have, which the next `sync`
/// removes.
pub fn edit(source: Option<&dyn Source>, root: &Path, edit: &Edit, flags: &SyncFlags) -> Report {
    let edited = match Edited::read(root, edit) {
        Ok(edited) => edited,
        Err(stopped) => return finish(Kind::Edit, Err(stopped), Vec::new()),
    };
    let Some(source) = source else {
        return edit_only(root, &edited);
    };
    let kind = if edited.unchanged() {
        Kind::Sync
    } else {
        Kind::Edit
    };
    let (run, drift) = match observe(root, &edited.applied.config) {
        Ok(Observation { active, old, drift }) => {
            let run = edit_stages(source, root, &edited, &active, old, flags);
            (run, drift)
        }
        Err(stopped) => (Err(stopped), Vec::new()),
    };
    finish(kind, run, drift)
}

fn edit_only(root: &Path, edited: &Edited) -> Report {
    if edited.unchanged() {
        return Report::new(Outcome::InSync, Vec::new());
    }
    let run = edited.write(root).map(|changes| Report {
        changes,
        ..Report::new(Outcome::InSync, Vec::new())
            .with_editor_hint(edited.first_active(), &edited.applied.config)
    });
    finish(Kind::NoSync, run, Vec::new())
}

/// Both stages of an edit, and the write of `refs.toml` between them.
fn edit_stages(
    source: &dyn Source,
    root: &Path,
    edited: &Edited,
    active: &ActiveSet,
    old: Option<Lock>,
    flags: &SyncFlags,
) -> Result<Report, Stopped> {
    let config = &edited.applied.config;
    let lock = match shrunk_lock(active, old.as_ref()) {
        Some(lock) => lock,
        None => resolve_all(source, active, old.as_ref(), flags)
            .map_err(|report| (Stop::StageOne, report))?,
    };
    let plan = stage_two_gate(source, root, config, active, &lock, flags.force)?;
    let lock_change = write_lock(root, old.as_ref(), &lock)?;
    let written = edited.write(root).map_err(|(stop, mut report)| {
        report.changes.extend(lock_change.clone());
        (stop, report)
    })?;
    let mut report = apply_plan(source, root, config, active, plan, flags)
        .with_editor_hint(edited.first_active(), config);
    report
        .changes
        .extend(written.into_iter().chain(lock_change));
    Ok(report)
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
) -> Result<Plan<'a>, Stopped> {
    match plan_stage_two(source, root, config, active, lock, force) {
        Ok(plan) if plan.refusals.is_empty() => Ok(plan),
        Ok(plan) => {
            let refusals = plan.refusals.into_iter().map(miette::Report::new);
            Err((
                Stop::Refused,
                Report::new(Outcome::Refused, refusals.collect()),
            ))
        }
        Err(errors) => Err((Stop::Unobserved, Report::new(Outcome::Failed, errors))),
    }
}

/// Write `lock` if it differs from the one on disk, and say so.
fn write_lock(root: &Path, old: Option<&Lock>, lock: &Lock) -> Result<Option<Changed>, Stopped> {
    if old == Some(lock) {
        return Ok(None);
    }
    lock.write(&Lock::path(root))
        .map_err(|e| (Stop::Lock, Report::failed(e)))?;
    Ok(Some(Changed::Lock))
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
        .map_err(|refusal| Report::new(Outcome::Refused, vec![refusal.into()]))?;
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
        Err(Report::new(Outcome::Failed, errors))
    }
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
    let outcome = plan.outcome(lock_drifted);
    let refusals = plan.refusals.into_iter().map(miette::Report::new);
    Report::new(outcome, refusals.collect())
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
    let mut outcome = if plan.refusals.is_empty() {
        Outcome::InSync
    } else {
        Outcome::Refused
    };
    // Only a removed Checkout is not listed in the block; a failed action of any other kind stales it.
    let mut held_back = false;
    let mut changes = Vec::new();
    for action in &plan.repos {
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
                held_back |= !matches!(action, RepoAction::Remove { .. });
            }
        }
    }
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
        ..Report::new(outcome, diagnostics)
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
