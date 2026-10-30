//! What a run says (ADR 0005, 0006): the `Report` and its changes, and the Hint, picked in
//! one table from the kind of run and where it stopped.

use crate::config::Config;
use crate::diagnostic::Note;
use crate::plan::{Checkout, Drift, Outcome};

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
    pub(super) fn new(outcome: Outcome, diagnostics: Vec<miette::Report>) -> Report {
        Report {
            outcome,
            changes: Vec::new(),
            drift: Vec::new(),
            diagnostics,
            hint: None,
        }
    }

    pub(super) fn failed(error: impl miette::Diagnostic + Send + Sync + 'static) -> Report {
        Report::new(Outcome::Failed, vec![miette::Report::new(error)])
    }

    /// The editor hint (spec 7.6), when `first` and the run got through.
    pub(super) fn with_editor_hint(mut self, first: bool, config: &Config) -> Report {
        if first && self.outcome == Outcome::InSync {
            let references_dir = config.settings.references_dir().to_owned();
            self.diagnostics
                .push(miette::Report::new(Note::EditorHint { references_dir }));
        }
        self
    }
}

/// Which run it was, for its Hint: what it does to `refs.toml`.
#[derive(Debug, Clone, Copy)]
pub(super) enum RunKind {
    /// `refs sync`, or an edit that changes nothing: `refs.toml` is as it was whatever
    /// happens next.
    KeepsConfig,
    /// An edit that changes `refs.toml`, then a sync.
    EditsConfig,
    /// An edit written without a sync (`--no-sync`).
    WritesConfigOnly,
}

/// A run that did not get through stage 2: where it stopped, with what it has to say.
pub(super) struct Stopped {
    pub(super) at: Stop,
    pub(super) report: Report,
}

/// Stop at `at` on `error`.
pub(super) fn failed_at<E>(at: Stop) -> impl FnOnce(E) -> Stopped
where
    E: miette::Diagnostic + Send + Sync + 'static,
{
    move |error| at.with(Report::failed(error))
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Stop {
    /// The edit could not be read or applied, or `refs.toml` could not be written.
    Rejected,
    /// `refs.lock` could not be read or written.
    LockFile,
    /// Stage 1 failed or was refused.
    StageOne,
    /// Stage 2 would refuse, so an edit was not written.
    Refused,
    /// Stage 2 could not be observed, so an edit was not written.
    Unobserved,
}

impl Stop {
    pub(super) fn with(self, report: Report) -> Stopped {
        Stopped { at: self, report }
    }
}

/// The follow-up for a `kind` of run that stopped at `stop` (`None`: it got through stage 2,
/// to `outcome`).
fn hint(kind: RunKind, stop: Option<Stop>, outcome: Outcome) -> Option<Hint> {
    let incomplete = matches!(outcome, Outcome::Failed | Outcome::Refused);
    match (kind, stop) {
        (_, Some(Stop::Rejected)) => Some(Hint::ConfigUnchanged),
        (RunKind::KeepsConfig, _) => incomplete.then_some(Hint::RunSyncOnceFixed),
        (RunKind::EditsConfig, Some(Stop::LockFile | Stop::Refused)) => Some(Hint::ConfigUnchanged),
        (RunKind::EditsConfig, Some(Stop::StageOne)) => Some(Hint::ConfigUnchangedTryNoSync),
        (RunKind::EditsConfig, Some(Stop::Unobserved)) => Some(Hint::ConfigUnchangedUnobserved),
        (RunKind::EditsConfig, None) => incomplete.then_some(Hint::FixOrRemoveThenSync),
        (RunKind::WritesConfigOnly, _) => Some(Hint::RunSync),
    }
}

/// The Report of a run: its Hint, and its changes in the order the CLI says them.
pub(super) fn finish(kind: RunKind, run: Result<Report, Stopped>, drift: Vec<Drift>) -> Report {
    let (stop, mut report) = match run {
        Ok(report) => (None, report),
        Err(Stopped { at, report }) => (Some(at), report),
    };
    report.hint = hint(kind, stop, report.outcome);
    report.changes.sort_by_key(Changed::rank);
    report.drift = drift;
    report
}
