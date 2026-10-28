//! The steps `cli::dispatch` strings together for a parsed command: load the project (`load`,
//! `load_or_create`), build the `Source` when the command syncs (`source`), then run and print
//! the report and give the exit code. What a command decides is in `sync`, `plan`, `add` and
//! `update`.

use std::path::Path;

use crate::add::Asking;
use crate::config::Config;
use crate::diagnostic::{Note, ProjectError};
use crate::edit::AddRepo;
use crate::project::{self, Project};
use crate::self_update::Updater;
use crate::source::{MakeSource, Source};
use crate::sync::SyncFlags;
use crate::ui::{
    Console, EXIT_OUT_OF_DATE, Prompter, fail, fail_all, failed, print_report, skipped,
};
use crate::update;

/// The project at or above `start`; on failure the problems are printed and the exit code
/// returned.
pub(crate) fn load(start: &Path, console: &mut Console) -> Result<Project, u8> {
    project::load(start).map_err(|reports| fail_all(reports, console))
}

/// `load`, except that with no `refs.toml` anywhere up the project is the place `refs init`
/// would set one up and an empty config: the edit creates the file (ADR 0005).
pub(crate) fn load_or_create(start: &Path, console: &mut Console) -> Result<Project, u8> {
    let no_config = matches!(
        project::find_root(start),
        Err(ProjectError::NoConfig { .. })
    );
    if !no_config {
        return load(start, console);
    }
    let root = project::init_root(start, false).map_err(|e| fail(e, console))?;
    Ok(Project {
        root,
        config: Config::default(),
    })
}

/// Build the `Source`; on failure the problem is printed and the exit code returned. Only the
/// commands that sync ask for one, so the others do not need a cache directory.
pub(crate) fn source<'a>(
    make_source: impl MakeSource<'a>,
    project: &Project,
    console: &mut Console,
) -> Result<Box<dyn Source + 'a>, u8> {
    make_source(&project.root, &project.config).map_err(|e| fail(e, console))
}

/// What `refs init` says after it created the config.
const REMINDERS: &str = "Add a repo with `refs add`, e.g. `refs add gh:owner/repo`.
Claude Code reads AGENTS.md only when there is no CLAUDE.md; to use CLAUDE.md, list it in `agents_files`.";

/// `refs init`: it needs no loaded project, as there may be none yet.
pub(crate) fn init(start: &Path, here: bool, console: &mut Console) -> u8 {
    let result = project::init_root(start, here)
        .map_err(miette::Report::new)
        .and_then(|root| crate::init::init(&root).map_err(miette::Report::new));
    match result {
        Ok(done) => {
            if !done.in_git {
                console.report(miette::Report::new(Note::NoGitRepo));
            }
            if done.config_created {
                console.status(format_args!("created {}", project::CONFIG_FILE));
                console.status(REMINDERS);
                console.advice(miette::Report::new(Note::EditorHint {
                    references_dir: done.references_dir,
                }));
            } else {
                console.status("nothing changed");
            }
            0
        }
        Err(report) => fail(report, console),
    }
}

/// `refs self update`: install the latest release over this one, or with `--check` only say
/// whether there is one (exit 3, as `sync --check` does for a project that is out of date).
pub(crate) fn self_update(updater: &dyn Updater, check: bool, console: &mut Console) -> u8 {
    if check {
        return match updater.is_update_available() {
            Ok(true) => {
                console
                    .problem("out of date: a newer release is available; run `refs self update`");
                EXIT_OUT_OF_DATE
            }
            Ok(false) => {
                console.status("up to date");
                0
            }
            Err(e) => fail(e, console),
        };
    }
    match updater.install() {
        Ok(Some(installed)) => {
            let from = installed
                .from
                .map_or(String::new(), |v| format!(" from {v}"));
            console.status(format_args!("updated refs{from} to {}", installed.to));
            0
        }
        Ok(None) => {
            console.status("up to date");
            0
        }
        Err(e) => fail(e, console),
    }
}

/// `refs add`, then its report. `source` is `None` for `--no-sync`.
pub(crate) fn add(
    source: Option<&dyn Source>,
    project: &Project,
    repo: &AddRepo,
    flags: &SyncFlags,
    mut asking: Asking,
    console: &mut Console,
) -> u8 {
    match crate::add::run(
        source,
        &project.root,
        &project.config,
        repo,
        flags,
        &mut asking,
        console,
    ) {
        Ok(report) => print_report(&report, false, console),
        Err(failure) => failed(failure, console),
    }
}

/// `refs update`, then its report and the count of the Repos it skipped.
pub(crate) fn update(
    source: &dyn Source,
    project: &Project,
    request: &update::Request,
    prompter: Option<&mut dyn Prompter>,
    console: &mut Console,
) -> u8 {
    match update::run(
        source,
        &project.root,
        &project.config,
        request,
        prompter,
        console,
    ) {
        Ok(done) => {
            console.data(&done.listing);
            let code = done
                .report
                .map_or(0, |report| print_report(&report, false, console));
            skipped(done.skipped, code, console)
        }
        Err(failure) => failed(failure, console),
    }
}

/// `refs list`.
pub(crate) fn list(project: &Project, color: bool, console: &mut Console) -> u8 {
    console.data(&crate::list::list(&project.config, color));
    0
}

/// `refs list --status`: the config, the Lock and the Checkout state of each active repo.
pub(crate) fn list_status(
    source: &dyn Source,
    project: &Project,
    color: bool,
    console: &mut Console,
) -> u8 {
    match crate::status::build(source, &project.root, &project.config) {
        Ok(status) => {
            console.data(&crate::list::list_status(
                &project.config,
                |laid| status.row(laid),
                color,
            ));
            0
        }
        Err(reports) => fail_all(reports, console),
    }
}
