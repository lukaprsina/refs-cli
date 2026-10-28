//! The steps `dispatch` strings together: load the project, build the `Source`, run a command and
//! print what it came to. Each returns the exit code.

use std::path::Path;

use crate::add;
use crate::edit::AddRepo;
use crate::list;
use crate::project::{self, Project};
use crate::registry::Registry;
use crate::source::{MakeSource, Source};
use crate::sync::SyncFlags;
use crate::ui::{self, Console, Prompter, fail, fail_all, failed, skipped};
use crate::update::{self, Done};

/// The project at or above `start`; on failure the problems are printed and the exit code
/// returned.
pub(super) fn load(start: &Path, console: &mut Console) -> Result<Project, u8> {
    project::load(start).map_err(|reports| fail_all(reports, console))
}

/// Build the `Source`; on failure the problem is printed and the exit code returned. Only the
/// commands that sync ask for one, so the others do not need a cache directory.
pub(super) fn build_source(
    make_source: impl MakeSource,
    project: &Project,
    console: &mut Console,
) -> Result<Box<dyn Source>, u8> {
    make_source(&project.root, &project.config).map_err(|e| fail(e, console))
}

/// `refs add`, then its report. `source` is `None` for `--no-sync`.
pub(super) fn run_add(
    source: Option<&dyn Source>,
    project: &Project,
    repo: &AddRepo,
    flags: &SyncFlags,
    prompter: Option<&mut dyn Prompter>,
    registry: &dyn Registry,
    console: &mut Console,
) -> u8 {
    match add::run(source, project, repo, flags, prompter, registry, console) {
        Ok(report) => ui::print_report(&report, false, console),
        Err(failure) => failed(failure, console),
    }
}

/// `refs update`, then its report and the count of the Repos it skipped.
pub(super) fn run_update(
    source: &dyn Source,
    project: &Project,
    request: &update::Request,
    prompter: Option<&mut dyn Prompter>,
    console: &mut Console,
) -> u8 {
    match update::run(source, project, request, prompter, console) {
        Ok(Done::Listed(listing)) => {
            console.data(&listing);
            0
        }
        Ok(Done::Ran {
            report,
            skipped: skipped_count,
        }) => {
            let code = report.map_or(0, |report| ui::print_report(&report, false, console));
            skipped(skipped_count, code, console)
        }
        Err(failure) => failed(failure, console),
    }
}

/// `refs list`, with the Lock and the Checkout state of each active repo when there is a
/// `source` (`--status`).
pub(super) fn run_list(
    project: &Project,
    source: Option<&dyn Source>,
    color: bool,
    console: &mut Console,
) -> u8 {
    match source
        .map(|source| list::status(source, project))
        .transpose()
    {
        Ok(status) => {
            console.data(&list::list(&project.config, status.as_ref(), color));
            0
        }
        Err(reports) => fail_all(reports, console),
    }
}
