//! The `sync` executor (ADR 0004): reads the project, runs the two stages, applies the plan.
//! Decisions live in `plan`; this module only does what the plans say and collects failures.

use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::Path;

use crate::active::{ActiveSet, active};
use crate::agent_file;
use crate::config::Config;
use crate::diagnostic::ProjectError;
use crate::lock::{Lock, LockedRepo};
use crate::plan::{
    Action, AgentFileText, Drift, Exclude, LockFlags, Plan, ProjectObserved, Step, lock_drift,
    plan_checkouts, plan_lock,
};
use crate::source::{MaterialiseOpts, Observed, Source};

const LOCK_FILE: &str = "refs.lock";

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

    fn failed(error: impl miette::Diagnostic + Send + Sync + 'static) -> Report {
        Report::new(Outcome::Failed, vec![], vec![miette::Report::new(error)])
    }
}

/// `refs lock`: stage 1 and a write.
pub fn lock(source: &dyn Source, root: &Path, config: &Config, flags: &LockFlags) -> Report {
    let active = active(config);
    let old = match read_lock(root) {
        Ok(old) => old,
        Err(e) => return Report::new(Outcome::Failed, vec![], vec![e]),
    };
    let drift = lock_drift(&active, old.as_ref());
    match stage_one(source, &active, old, flags, root) {
        Ok(_) => Report::new(Outcome::InSync, drift, vec![]),
        Err(report) => report,
    }
}

/// `refs sync`. With `check`, nothing is resolved, verified, fetched or written.
pub fn sync(source: &dyn Source, root: &Path, config: &Config, flags: &SyncFlags) -> Report {
    let active = active(config);
    let old = match read_lock(root) {
        Ok(old) => old,
        Err(e) => return Report::new(Outcome::Failed, vec![], vec![e]),
    };
    let drift = lock_drift(&active, old.as_ref());
    let lock = if flags.check {
        old
    } else {
        let lock_flags = LockFlags {
            offline: flags.offline,
            ..LockFlags::default()
        };
        match stage_one(source, &active, old, &lock_flags, root) {
            Ok(lock) => Some(lock),
            Err(report) => return Report { drift, ..report },
        }
    };
    let mut diagnostics = Vec::new();
    let plan = lock.as_ref().and_then(|lock| {
        match plan_stage_two(source, root, config, &active, lock, flags.force) {
            Ok(plan) => plan,
            Err(errors) => {
                diagnostics.extend(errors);
                None
            }
        }
    });
    if !diagnostics.is_empty() {
        return Report::new(Outcome::Failed, drift, diagnostics);
    }
    match plan {
        Some(plan) if flags.check => check(plan, drift),
        Some(plan) => {
            let dir = config.settings.references_dir();
            apply(source, root, &active, dir, plan, flags, drift)
        }
        // `--check` without a Lock to plan against: the Lock is what is out of date.
        None => Report::new(Outcome::OutOfDate, drift, diagnostics),
    }
}

fn read_lock(root: &Path) -> Result<Option<Lock>, miette::Report> {
    match std::fs::read_to_string(root.join(LOCK_FILE)) {
        Ok(text) => Ok(Some(Lock::parse(&text)?)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ProjectError::Read {
            path: LOCK_FILE.into(),
            source,
        }
        .into()),
    }
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
                    paths: repo.repo.path_strings(),
                }),
                Err(e) => errors.push(e.into()),
            },
        }
    }
    for entry in &entries {
        let repo = active
            .get(&entry.id)
            .expect("a step is made per active Repo");
        if let Err(e) = source.verify(repo, &entry.pin) {
            errors.push(e.into());
        }
    }
    if !errors.is_empty() {
        return Err(Report::new(Outcome::Failed, vec![], errors));
    }
    let lock = Lock::new(entries);
    if old.as_ref() != Some(&lock)
        && let Err(e) = lock.write(&root.join(LOCK_FILE))
    {
        return Err(Report::failed(e));
    }
    Ok(lock)
}

/// Read the project and what is on disk, then plan the checkouts.
fn plan_stage_two(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    active: &ActiveSet,
    lock: &Lock,
    force: bool,
) -> Result<Option<Plan>, Vec<miette::Report>> {
    let mut errors: Vec<miette::Report> = Vec::new();
    let listing = source.list().unwrap_or_else(|e| {
        errors.push(e.into());
        vec![]
    });
    let mut observed: HashMap<String, Observed> = HashMap::new();
    let names = active
        .repos()
        .map(|r| r.id.to_string())
        .chain(listing.iter().filter(|n| active.get(n).is_none()).cloned());
    for name in names {
        match source.inspect(&name) {
            Ok(o) => {
                observed.insert(name, o);
            }
            Err(e) => errors.push(e.into()),
        }
    }
    let project = match read_project(root, config, listing) {
        Ok(project) => Some(project),
        Err(e) => {
            errors.push(e);
            None
        }
    };
    let Some(project) = project.filter(|_| errors.is_empty()) else {
        return Err(errors);
    };
    // A Lock that does not cover the active set can only be seen by `--check`, which does
    // not repair it: the Lock is what is out of date.
    Ok(plan_checkouts(active, lock, &observed, &project, force).ok())
}

fn read_project(
    root: &Path,
    config: &Config,
    listing: Vec<String>,
) -> Result<ProjectObserved, miette::Report> {
    let references_dir = config.settings.references_dir().to_string();
    let files = config.settings.agents_files();
    let mut agent_files = Vec::new();
    for path in files {
        let text = match std::fs::read_to_string(root.join(&path)) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            Err(source) => return Err(ProjectError::Read { path, source }.into()),
        };
        agent_files.push(AgentFileText { path, text });
    }
    let exclude = read_exclude(root, &references_dir)?;
    Ok(ProjectObserved {
        references_dir,
        agent_files,
        listing,
        exclude,
    })
}

fn exclude_line(references_dir: &str) -> String {
    format!("/{references_dir}/")
}

fn read_exclude(root: &Path, references_dir: &str) -> Result<Exclude, miette::Report> {
    let git = root.join(".git");
    if !git.is_dir() {
        return Ok(Exclude::NoGit);
    }
    let path = git.join("info/exclude");
    match std::fs::read_to_string(&path) {
        Ok(text)
            if text
                .lines()
                .any(|l| l.trim() == exclude_line(references_dir)) =>
        {
            Ok(Exclude::Present)
        }
        Ok(_) => Ok(Exclude::Missing),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Exclude::Missing),
        Err(source) => Err(ProjectError::Read {
            path: path.display().to_string(),
            source,
        }
        .into()),
    }
}

fn ensure_exclude(root: &Path, references_dir: &str) -> Result<(), ProjectError> {
    let path = root.join(".git/info/exclude");
    let write = |source| ProjectError::Write {
        path: path.display().to_string(),
        source,
    };
    let mut text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(source) => return Err(write(source)),
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&exclude_line(references_dir));
    text.push('\n');
    std::fs::create_dir_all(path.parent().expect("a file has a parent")).map_err(write)?;
    crate::atomic::write(&path, &text).map_err(write)
}

/// `--check`: report what applying the plan would do.
fn check(plan: Plan, drift: Vec<Drift>) -> Report {
    let refused = plan.refusals().next().is_some();
    let out_of_date = plan.is_drift() || !drift.is_empty();
    let outcome = match () {
        _ if refused => Outcome::Refused,
        _ if out_of_date => Outcome::OutOfDate,
        _ => Outcome::InSync,
    };
    let diagnostics = plan
        .actions
        .into_iter()
        .filter_map(|a| match a {
            Action::Refuse(r) => Some(miette::Report::new(r)),
            _ => None,
        })
        .collect();
    Report::new(outcome, drift, diagnostics)
}

/// Apply the plan in order. A failure does not stop the other Repos, but nothing that
/// depends on a failed Repo is done: not its materialise after a failed remove, and no
/// Agent file write at all.
fn apply(
    source: &dyn Source,
    root: &Path,
    active: &ActiveSet,
    references_dir: &str,
    plan: Plan,
    flags: &SyncFlags,
    drift: Vec<Drift>,
) -> Report {
    let opts = MaterialiseOpts {
        offline: flags.offline,
    };
    let mut diagnostics: Vec<miette::Report> = Vec::new();
    let mut failed_ids: HashSet<String> = HashSet::new();
    let mut failed = false;
    let mut refused = false;
    for action in plan.actions {
        match action {
            Action::Remove { id } => {
                if let Err(e) = source.remove(&id) {
                    diagnostics.push(e.into());
                    failed_ids.insert(id);
                }
            }
            Action::Materialise { id, pin } => {
                let repo = active.get(&id).expect("only active Repos are materialised");
                if failed_ids.contains(&id) {
                    continue;
                }
                if let Err(e) = source.materialise(repo, &pin, opts) {
                    diagnostics.push(e.into());
                    failed_ids.insert(id);
                }
            }
            Action::Refuse(r) => {
                refused = true;
                diagnostics.push(r.into());
            }
            Action::Note(n) => {
                let skipped = matches!(&n, crate::diagnostic::Note::Recreated { id } if failed_ids.contains(id));
                if !skipped {
                    diagnostics.push(n.into());
                }
            }
            Action::WriteAgentFile { path, text } => {
                if !failed_ids.is_empty() || failed {
                    continue;
                }
                if let Err(e) = agent_file::write(&root.join(&path), &text) {
                    diagnostics.push(e.into());
                    failed = true;
                }
            }
            Action::EnsureExclude => {
                if let Err(e) = ensure_exclude(root, references_dir) {
                    diagnostics.push(e.into());
                    failed = true;
                }
            }
        }
    }
    let outcome = match () {
        _ if failed || !failed_ids.is_empty() => Outcome::Failed,
        _ if refused => Outcome::Refused,
        _ => Outcome::InSync,
    };
    Report::new(outcome, drift, diagnostics)
}
