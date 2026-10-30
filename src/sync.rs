//! The `sync` executor (ADR 0006): reads the project, runs the typed actions of the two
//! plans through `Source`, and collects what happened into a `Report`. Which names are
//! observed, how Lock entries are built, and the outcome of `--check` are decided in `plan`;
//! the Hint is picked in `report`, from where a run stopped.

mod report;

use std::path::Path;

use crate::active::{ActiveSet, active};
use crate::agent_file;
use crate::config::{self, Config};
use crate::diagnostic::Note;
use crate::edit::{self, Edit};
use crate::exclude;
use crate::lock::{Lock, LockedRepo};
use crate::outline;
pub use crate::plan::{Checkout, Outcome};
use crate::plan::{
    Checkouts, Drift, Exclude, Plan, RepoAction, Step, lock_drift, plan_checkouts, plan_lock,
    shrunk_lock,
};
use crate::project;
use crate::source::Source;
use crate::tooling;
pub use report::{Changed, Hint, Report};
use report::{RunKind, Stop, Stopped, failed_at, finish};

#[derive(Debug, Default)]
pub struct SyncFlags {
    pub offline: bool,
    pub force: bool,
    pub check: bool,
    /// Floating Repos to re-resolve in the Lock. `update` sets it; `sync` has no such flag, but
    /// reads it all the same (through `resolve_all`).
    pub reresolve: Vec<String>,
}

/// Read the Lock on disk (if any), see how it differs from the active set of `config`, run
/// `stages` on the three, and build the Report.
fn observe_and_run(
    kind: RunKind,
    root: &Path,
    config: &Config,
    stages: impl FnOnce(&ActiveSet, Option<Lock>, &[Drift]) -> Result<Report, Stopped>,
) -> Report {
    let active = active(config);
    let old = match Lock::read(&Lock::path(root)) {
        Ok(old) => old,
        Err(e) => return finish(kind, Err(failed_at(Stop::LockFile)(e)), Vec::new()),
    };
    let drift = lock_drift(&active, old.as_ref());
    let run = stages(&active, old, &drift);
    finish(kind, run, drift)
}

/// `refs sync`. With `check`, nothing is resolved, verified, fetched or written.
pub fn sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    observe_and_run(RunKind::KeepsConfig, root, config, |active, old, drift| {
        run_sync(source, root, config, active, old, drift, flags)
    })
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
            resolve_all(source, active, old.as_ref(), flags).map_err(|r| Stop::StageOne.with(r))?;
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
            false => project::read_config(root).map_err(failed_at(Stop::Rejected))?,
        };
        let applied = edit.apply(&before).map_err(failed_at(Stop::Rejected))?;
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
        project::write_config(root, &self.applied.text).map_err(failed_at(Stop::Rejected))?;
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
        // A rejected edit is hinted alike whatever the kind.
        Err(stopped) => return finish(RunKind::EditsConfig, Err(stopped), Vec::new()),
        Ok(edited) => edited,
    };
    let kind = match (source, edited.unchanged()) {
        (_, true) => RunKind::KeepsConfig,
        (Some(_), false) => RunKind::EditsConfig,
        (None, false) => RunKind::WritesConfigOnly,
    };
    let config = &edited.applied.config;
    match source {
        Some(source) => observe_and_run(kind, root, config, |active, old, _| {
            edit_stages(source, root, &edited, active, old, flags)
        }),
        None => {
            let run = edited.write(root).map(|changes| Report {
                changes,
                ..Report::new(Outcome::InSync, Vec::new())
                    .with_editor_hint(edited.first_active(), config)
            });
            finish(kind, run, Vec::new())
        }
    }
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
        None => {
            resolve_all(source, active, old.as_ref(), flags).map_err(|r| Stop::StageOne.with(r))?
        }
    };
    let plan = stage_two_gate(source, root, config, active, &lock, flags.force)?;
    let lock_change = write_lock(root, old.as_ref(), &lock)?;
    let written = match edited.write(root) {
        Ok(written) => written,
        Err(mut stopped) => {
            stopped.report.changes.extend(lock_change);
            return Err(stopped);
        }
    };
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
        // What `--check` would say of it: refused, with the refusals.
        Ok(plan) => Err(Stop::Refused.with(check(plan, false))),
        Err(errors) => Err(Stop::Unobserved.with(Report::new(Outcome::Failed, errors))),
    }
}

/// Write `lock` if it differs from the one on disk, and say so.
fn write_lock(root: &Path, old: Option<&Lock>, lock: &Lock) -> Result<Option<Changed>, Stopped> {
    if old == Some(lock) {
        return Ok(None);
    }
    lock.write(&Lock::path(root))
        .map_err(failed_at(Stop::LockFile))?;
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
        match source.files(repo, &entry.pin, flags.offline) {
            Ok(files) => {
                let wrong = outline::verify(repo, &entry.pin, &files);
                if wrong.is_empty() {
                    passed.push(entry);
                }
                errors.extend(wrong.into_iter().map(|e| e.for_repo(repo.id)));
            }
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
