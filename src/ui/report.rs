//! The words of a run's ending: the report of a sync, the hint after it, why a command stopped,
//! and the exit code for each.

use super::Abort;
use super::console::{Console, EXIT_ERROR, EXIT_OUT_OF_DATE, EXIT_USAGE, fail};
use crate::failure::Failure;
use crate::project;
use crate::sync::{Changed, Checkout, Hint, Outcome, Report};

/// Say why a question ended without an answer, and give the exit code.
fn aborted(abort: Abort, console: &mut Console) -> u8 {
    match abort {
        Abort::Cancelled => console.problem("cancelled"),
        Abort::Failed(why) => console.problem(format_args!("cannot prompt: {why}")),
    }
    EXIT_ERROR
}

/// Say why `add` or `update` stopped, and give the exit code.
pub(crate) fn failed(failure: Failure, console: &mut Console) -> u8 {
    match failure {
        Failure::Error(report) => fail(report, console),
        Failure::Asked(abort) => aborted(abort, console),
        Failure::Usage(report) => {
            fail(report, console);
            EXIT_USAGE
        }
    }
}

/// The words of a hint (ADR 0005). Not a status line: the project is incomplete or failed,
/// so `-q` does not hide it.
fn say_hint(hint: Hint, console: &mut Console) {
    match hint {
        Hint::RunSync => console.problem("run `refs sync` to bring the project up to date"),
        Hint::RunSyncOnceFixed => console.problem("run `refs sync` once the problem is fixed"),
        Hint::FixOrRemoveThenSync => console.problem(format_args!(
            "{} was updated; fix or remove the broken repo, then run `refs sync`",
            project::CONFIG_FILE
        )),
        Hint::ConfigUnchanged => {
            console.problem(format_args!("{} was not changed", project::CONFIG_FILE))
        }
        Hint::ConfigUnchangedUnobserved => console.problem(format_args!(
            "{} was not changed; fix what could not be read, or pass `--no-sync` to edit without syncing",
            project::CONFIG_FILE
        )),
        Hint::ConfigUnchangedTryNoSync => console.problem(format_args!(
            "{} was not changed; fix the broken repo, or pass `--no-sync` to edit without syncing",
            project::CONFIG_FILE
        )),
    }
}

fn exit_code(outcome: Outcome) -> u8 {
    match outcome {
        Outcome::InSync => 0,
        Outcome::OutOfDate => EXIT_OUT_OF_DATE,
        Outcome::Refused | Outcome::Failed => EXIT_ERROR,
    }
}

/// Say how many Repos `refs update` left alone, and give the exit code: `code` (the sync's), or
/// an error when any were skipped.
pub(crate) fn skipped(count: usize, code: u8, console: &mut Console) -> u8 {
    if count == 0 {
        return code;
    }
    console.problem(match count {
        1 => "1 repo was not updated".to_owned(),
        n => format!("{n} repos were not updated"),
    });
    code.max(EXIT_ERROR)
}

/// One status line per change, the problems, and "nothing changed" for a run that did no
/// change and had no problem (`--check` changes nothing by design: it says "up to date").
/// Gives the exit code.
pub(crate) fn print_report(report: &Report, check: bool, console: &mut Console) -> u8 {
    for change in &report.changes {
        match change {
            Changed::Config => console.status(format_args!("updated {}", project::CONFIG_FILE)),
            Changed::ConfigCreated => {
                console.status(format_args!("created {}", project::CONFIG_FILE))
            }
            Changed::Group(group) => console.status(format_args!("created group `{group}`")),
            Changed::Lock => console.status(format_args!("updated {}", crate::lock::FILE)),
            Changed::Checkout { id, how } => {
                let how = match how {
                    Checkout::Created => "created",
                    Checkout::Moved => "moved",
                    Checkout::Removed => "removed",
                };
                console.status(format_args!("{how} checkout `{id}`"));
            }
            Changed::AgentFile(path) => console.status(format_args!("updated {path}")),
        }
    }
    for diagnostic in &report.diagnostics {
        if console.quiet() && report.outcome == Outcome::InSync {
            continue;
        }
        console.report(diagnostic);
    }
    if report.outcome == Outcome::OutOfDate {
        for drift in &report.drift {
            console.problem(format_args!("out of date: {drift}"));
        }
    }
    if report.outcome == Outcome::InSync && report.changes.is_empty() {
        console.status(if check {
            "up to date"
        } else {
            "nothing changed"
        });
    }
    if let Some(hint) = report.hint {
        say_hint(hint, console);
    }
    exit_code(report.outcome)
}
