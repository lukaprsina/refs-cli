//! The command line: parsing, output and exit codes. No logic (architecture.md): every
//! decision is made by `sync` and `plan`.
//!
//! The `Source` is injected as an argument to `run`: tests pass the fake in-process (the
//! `testing` feature), and `main` passes a stub until `GitSource` exists. There is no
//! hidden flag, so the shipped binary cannot be pointed at a fake.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::plan::{Drift, LockFlags, Upgrade};
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
/// code: 0 in sync, 1 error or refusal, 2 usage, 3 `--check` found drift.
pub fn run(args: impl IntoIterator<Item = OsString>, cwd: &Path, source: &dyn Source) -> u8 {
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
    };
    print(&report, cli.quiet);
    exit_code(report.outcome)
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
            eprintln!("out of date: {}", describe(drift));
        }
        eprintln!("run `refs sync` to bring the project up to date");
    }
}

fn describe(drift: &Drift) -> String {
    match drift {
        Drift::LockMissing => "refs.lock is missing".into(),
        Drift::Added(id) => format!("`{id}` is not locked"),
        Drift::Removed(id) => format!("`{id}` is locked but no longer active"),
        Drift::Changed { id, field } => format!("`{id}` changed its {field}"),
    }
}
