//! The command line: parsing, output and exit codes. No logic (architecture.md): every
//! decision is made by `sync` and `plan`.
//!
//! The `Source` is injected into `run` as a function of the loaded project (the real one
//! needs its root and `references_dir`): tests return the fake in-process (the `testing`
//! feature), and `main` builds a `GitSource`. There is no hidden flag, so the shipped binary
//! cannot be pointed at a fake.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::atomic;
use crate::config::Config;
use crate::diagnostic::{EditError, ProjectError, SourceError};
use crate::edit::{self, AddRepo};
use crate::list::list;
use crate::plan::{LockFlags, Upgrade};
use crate::project;
use crate::source::Source;
use crate::sync::{self, Outcome, Report, SyncFlags};

const EXIT_ERROR: u8 = 1;
const EXIT_OUT_OF_DATE: u8 = 3;

#[derive(Debug, Parser)]
#[command(name = "refs", version, about)]
pub struct Cli {
    /// The project directory, instead of searching upwards from the current one
    #[arg(long, global = true, value_name = "DIR")]
    project: Option<PathBuf>,
    /// Print nothing on a successful run (notes are hidden); problems are always printed
    #[arg(short, long, global = true)]
    quiet: bool,
    /// Plain output, no colors or box drawing
    #[arg(long, global = true)]
    no_color: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Resolve refs to commits and write refs.lock
    Lock(LockArgs),
    /// Make the checkouts and the managed block match refs.lock
    Sync(SyncArgs),
    /// List the repos in refs.toml
    List,
    /// Add a repo to refs.toml
    Add(AddArgs),
    /// Remove a repo from refs.toml
    Remove(RemoveArgs),
}

#[derive(Debug, Args)]
struct AddArgs {
    /// The repository to add
    url: String,
    /// The repo's id, instead of the last segment of the URL
    #[arg(long)]
    id: Option<String>,
    /// The group to put it in; it must exist
    #[arg(long)]
    group: Option<String>,
    /// A branch, tag or full commit id; the remote's default branch when absent
    #[arg(long = "ref", value_name = "REF")]
    git_ref: Option<String>,
    /// Repo-relative directories to check out
    #[arg(long, num_args = 1.., value_name = "PATH")]
    paths: Vec<String>,
    /// Names, as imported in code, of the packages the repo documents or implements
    #[arg(long, num_args = 1.., value_name = "PACKAGE")]
    packages: Vec<String>,
    /// Repo-relative files worth reading first
    #[arg(long, num_args = 1.., value_name = "PATH")]
    start: Vec<String>,
    /// One line on what the repo is
    #[arg(long)]
    description: Option<String>,
}

#[derive(Debug, Args)]
struct RemoveArgs {
    /// The id of the repo to remove
    id: String,
}

#[derive(Debug, Args)]
struct LockArgs {
    /// Re-resolve floating refs: all of them, or only the given repos
    #[arg(long, num_args = 0.., value_name = "ID")]
    upgrade: Option<Vec<String>>,
}

#[derive(Debug, Args)]
struct SyncArgs {
    /// Change nothing; exit 3 if the lock, checkouts or blocks are out of date
    #[arg(long)]
    check: bool,
    /// Never contact the network
    #[arg(long)]
    offline: bool,
    /// Discard local changes in checkouts
    #[arg(long)]
    force: bool,
}

impl LockArgs {
    fn upgrade(&self) -> Upgrade {
        match &self.upgrade {
            None => Upgrade::None,
            Some(ids) if ids.is_empty() => Upgrade::All,
            Some(ids) => Upgrade::Ids(ids.clone()),
        }
    }
}

/// Run `refs` with `args` (the first is the program name) from `cwd`, and return the exit
/// code: 0 in sync, 1 error or refusal, 2 usage, 3 `--check` found drift. `make_source` is
/// given the project root and config once they are loaded.
pub fn run<'a>(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
) -> u8 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return u8::try_from(e.exit_code()).unwrap_or(EXIT_ERROR);
        }
    };
    install_report_handler(cli.no_color);
    let start = cli.project.as_deref().unwrap_or(cwd);
    let (root, config) = match project::load(start) {
        Ok(loaded) => loaded,
        Err(reports) => {
            for report in reports {
                eprintln!("{report:?}");
            }
            return EXIT_ERROR;
        }
    };
    // These commands read and edit `refs.toml` only, so they never get a `Source`.
    match &cli.command {
        Command::List => {
            print!("{}", list(&config));
            return 0;
        }
        Command::Add(args) => {
            return edit_config(&root, cli.quiet, |text| edit::add(text, &args.into()));
        }
        Command::Remove(args) => {
            return edit_config(&root, cli.quiet, |text| edit::remove(text, &args.id));
        }
        Command::Lock(_) | Command::Sync(_) => {}
    }
    let source = match make_source(&root, &config) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("{:?}", miette::Report::new(e));
            return EXIT_ERROR;
        }
    };
    let source = &*source;
    let report = match &cli.command {
        Command::Lock(args) => {
            let flags = LockFlags {
                upgrade: args.upgrade(),
                offline: false,
            };
            sync::lock(source, &root, &config, &flags)
        }
        Command::Sync(args) => {
            let flags = SyncFlags {
                offline: args.offline,
                force: args.force,
                check: args.check,
            };
            sync::sync(source, &root, &config, &flags)
        }
        Command::List | Command::Add(_) | Command::Remove(_) => {
            unreachable!("handled before the Source is made")
        }
    };
    print(&report, cli.quiet);
    exit_code(report.outcome)
}

impl From<&AddArgs> for AddRepo {
    fn from(args: &AddArgs) -> AddRepo {
        AddRepo {
            url: args.url.clone(),
            id: args.id.clone(),
            group: args.group.clone(),
            git_ref: args.git_ref.clone(),
            description: args.description.clone(),
            paths: args.paths.clone(),
            packages: args.packages.clone(),
            start: args.start.clone(),
        }
    }
}

/// Apply `edit` to the text of `<root>/refs.toml` and write the result atomically. A
/// rejected edit writes nothing.
fn edit_config(
    root: &Path,
    quiet: bool,
    edit: impl FnOnce(&str) -> Result<String, EditError>,
) -> u8 {
    let path = root.join(project::CONFIG_FILE);
    let result = std::fs::read_to_string(&path)
        .map_err(|source| ProjectError::Read {
            path: project::CONFIG_FILE.into(),
            source,
        })
        .map_err(miette::Report::new)
        .and_then(|text| edit(&text).map_err(miette::Report::new))
        .and_then(|text| {
            atomic::write(&path, &text).map_err(|source| {
                miette::Report::new(ProjectError::Write {
                    path: project::CONFIG_FILE.into(),
                    source,
                })
            })
        });
    match result {
        Ok(()) => {
            if !quiet {
                println!(
                    "updated {}; run `refs sync` to bring the project up to date",
                    project::CONFIG_FILE
                );
            }
            0
        }
        Err(report) => {
            eprintln!("{report:?}");
            EXIT_ERROR
        }
    }
}

fn exit_code(outcome: Outcome) -> u8 {
    match outcome {
        Outcome::InSync => 0,
        Outcome::OutOfDate => EXIT_OUT_OF_DATE,
        Outcome::Refused | Outcome::Failed => EXIT_ERROR,
    }
}

fn install_report_handler(no_color: bool) {
    if no_color {
        // Fails only when a handler is already installed, as on a second in-process run.
        let _ = miette::set_hook(Box::new(|_| {
            Box::new(miette::GraphicalReportHandler::new_themed(
                miette::GraphicalTheme::unicode_nocolor(),
            ))
        }));
    }
}

fn print(report: &Report, quiet: bool) {
    for diagnostic in &report.diagnostics {
        if quiet && report.outcome == Outcome::InSync {
            continue;
        }
        eprintln!("{diagnostic:?}");
    }
    if report.outcome == Outcome::OutOfDate {
        for drift in &report.drift {
            eprintln!("out of date: {drift}");
        }
        eprintln!("run `refs sync` to bring the project up to date");
    }
}
