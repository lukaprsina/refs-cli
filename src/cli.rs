//! The command line: parsing, output and exit codes. No logic (architecture.md): every
//! decision is made by `sync` and `plan`.
//!
//! The `Source` is injected into `run` as a function of the loaded project (the real one
//! needs its root and `references_dir`): tests return the fake in-process (the `testing`
//! feature), and `main` builds a `GitSource`. There is no hidden flag, so the shipped binary
//! cannot be pointed at a fake.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::config::Config;
use crate::diagnostic::{EditError, SourceError};
use crate::edit::{self, AddRepo, Target};
use crate::list::{Status, list};
use crate::lock::Lock;
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

/// The commands, by what they need: `init` runs where there may be no project yet, the
/// config commands read and edit `refs.toml` and ask for a `Source` only to sync after an
/// edit (unless `--no-sync`) or to show `list --status`, and the rest act on the project
/// through one.
#[derive(Debug, Subcommand)]
enum Command {
    /// Set up a project: refs.toml, the managed block and the git exclude rule
    Init(InitArgs),
    #[command(flatten)]
    Config(ConfigCommand),
    #[command(flatten)]
    Source(SourceCommand),
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// List the repos in refs.toml
    List(ListArgs),
    /// Add a repo to refs.toml, lock it and sync
    Add {
        #[command(flatten)]
        repo: AddRepo,
        #[command(flatten)]
        flags: EditFlags,
    },
    /// Remove a repo from refs.toml
    Remove(RemoveArgs),
    /// Stop tracking a repo, or a group, without removing it
    Disable(ToggleArgs),
    /// Track a disabled repo or group again
    Enable(ToggleArgs),
}

#[derive(Debug, Subcommand)]
enum SourceCommand {
    /// Resolve refs to commits and write refs.lock
    Lock(LockArgs),
    /// Make the checkouts and the managed block match refs.lock
    Sync(SyncArgs),
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Also show the locked commit and the state of each checkout
    #[arg(long)]
    status: bool,
}

/// What every command that edits `refs.toml` takes.
#[derive(Debug, Args)]
struct EditFlags {
    /// Only edit refs.toml; do not lock or sync
    #[arg(long)]
    no_sync: bool,
}

#[derive(Debug, Args)]
struct ToggleArgs {
    #[command(flatten)]
    flags: EditFlags,
    /// The id of the repo, or of the group with --group
    id: String,
    /// The id names a group
    #[arg(long)]
    group: bool,
}

impl ToggleArgs {
    fn target(&self) -> Target<'_> {
        if self.group {
            Target::Group(&self.id)
        } else {
            Target::Repo(&self.id)
        }
    }
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Create a project here even below a directory that already has a refs.toml
    #[arg(long)]
    here: bool,
}

#[derive(Debug, Args)]
struct RemoveArgs {
    /// The id of the repo to remove
    id: String,
    #[command(flatten)]
    flags: EditFlags,
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
    let quiet = cli.quiet;
    match cli.command {
        Command::Init(args) => init(start, args.here, quiet),
        Command::Config(command) => match load(start) {
            Ok((root, config)) => run_config(&command, &root, &config, quiet, make_source),
            Err(code) => code,
        },
        Command::Source(command) => match load(start) {
            Ok((root, config)) => run_source(&command, &root, &config, quiet, make_source),
            Err(code) => code,
        },
    }
}

/// The project at or above `start`; on failure the problems are printed and the exit code
/// returned.
fn load(start: &Path) -> Result<(PathBuf, Config), u8> {
    project::load(start).map_err(|reports| {
        for report in reports {
            eprintln!("{report:?}");
        }
        EXIT_ERROR
    })
}

fn run_config<'a>(
    command: &ConfigCommand,
    root: &Path,
    config: &Config,
    quiet: bool,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
) -> u8 {
    type Edit<'e> = Box<dyn Fn(&str) -> Result<String, EditError> + 'e>;
    let (flags, edit): (&EditFlags, Edit) = match command {
        ConfigCommand::List(args) if args.status => {
            return list_status(root, config, make_source);
        }
        ConfigCommand::List(_) => {
            print!("{}", list(config));
            return 0;
        }
        ConfigCommand::Add { repo, flags } => (flags, Box::new(|text| edit::add(text, repo))),
        ConfigCommand::Remove(args) => (&args.flags, Box::new(|text| edit::remove(text, &args.id))),
        ConfigCommand::Disable(args) => (
            &args.flags,
            Box::new(|text| edit::disable(text, args.target())),
        ),
        ConfigCommand::Enable(args) => (
            &args.flags,
            Box::new(|text| edit::enable(text, args.target())),
        ),
    };
    edit_config(root, config, quiet, flags, make_source, &*edit)
}

/// `refs list --status`: the config, the Lock and what `inspect` finds for every repo.
fn list_status<'a>(
    root: &Path,
    config: &Config,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
) -> u8 {
    let result = (|| -> Result<String, miette::Report> {
        let source = make_source(root, config).map_err(miette::Report::new)?;
        let lock = Lock::read(&Lock::path(root)).map_err(miette::Report::new)?;
        let mut observed = HashMap::new();
        for id in config.repos.keys() {
            let id = id.as_ref().as_str();
            let seen = source.inspect(id).map_err(miette::Report::new)?;
            observed.insert(id.to_string(), seen);
        }
        let status = Status {
            lock: lock.as_ref(),
            observed: &observed,
        };
        Ok(crate::list::list_status(config, &status))
    })();
    match result {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(report) => {
            eprintln!("{report:?}");
            EXIT_ERROR
        }
    }
}

fn run_source<'a>(
    command: &SourceCommand,
    root: &Path,
    config: &Config,
    quiet: bool,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
) -> u8 {
    let source = match make_source(root, config) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("{:?}", miette::Report::new(e));
            return EXIT_ERROR;
        }
    };
    let source = &*source;
    let report = match command {
        SourceCommand::Lock(args) => {
            let flags = LockFlags {
                upgrade: args.upgrade(),
                offline: false,
            };
            sync::lock(source, root, config, &flags)
        }
        SourceCommand::Sync(args) => {
            let flags = SyncFlags {
                offline: args.offline,
                force: args.force,
                check: args.check,
            };
            sync::sync(source, root, config, &flags)
        }
    };
    print(&report, quiet);
    exit_code(report.outcome)
}

/// Apply `edit` to the text of `<root>/refs.toml`. With `--no-sync` write the result and say
/// to run `refs sync`; otherwise lock the edited config first and write it only if that
/// passes, then sync (`sync::sync_edited`). A rejected edit writes nothing.
fn edit_config<'a>(
    root: &Path,
    config: &Config,
    quiet: bool,
    flags: &EditFlags,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    edit: &dyn Fn(&str) -> Result<String, EditError>,
) -> u8 {
    let edited = project::read_config(root)
        .map_err(miette::Report::new)
        .and_then(|text| Ok((edit(&text).map_err(miette::Report::new)?, text)));
    let (edited, text) = match edited {
        Ok(both) => both,
        Err(report) => {
            eprintln!("{report:?}");
            return EXIT_ERROR;
        }
    };
    if flags.no_sync {
        return write_only(root, quiet, &text, &edited);
    }
    let source = match make_source(root, config) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("{:?}", miette::Report::new(e));
            return EXIT_ERROR;
        }
    };
    let (written, report) = sync::sync_edited(&*source, root, &edited, &SyncFlags::default());
    if written && !quiet {
        println!("updated {}", project::CONFIG_FILE);
    }
    print(&report, quiet);
    let failed = matches!(report.outcome, Outcome::Failed | Outcome::Refused);
    if failed && written {
        eprintln!(
            "{} and refs.lock were updated; run `refs sync` once the problem is fixed",
            project::CONFIG_FILE
        );
    } else if failed {
        eprintln!("{} was not changed", project::CONFIG_FILE);
    }
    exit_code(report.outcome)
}

fn write_only(root: &Path, quiet: bool, text: &str, edited: &str) -> u8 {
    if edited == text {
        if !quiet {
            println!("{} already says that", project::CONFIG_FILE);
        }
        return 0;
    }
    match project::write_config(root, edited) {
        Ok(()) => {
            if !quiet {
                println!(
                    "updated {}; run `refs sync` to bring the project up to date",
                    project::CONFIG_FILE
                );
            }
            0
        }
        Err(e) => {
            eprintln!("{:?}", miette::Report::new(e));
            EXIT_ERROR
        }
    }
}

/// `refs init`: it needs no loaded project, as there may be none yet.
fn init(start: &Path, here: bool, quiet: bool) -> u8 {
    let result = project::init_root(start, here)
        .map_err(miette::Report::new)
        .and_then(|root| crate::init::init(&root).map_err(miette::Report::new));
    match result {
        Ok(done) => {
            if !quiet {
                if done.config_created {
                    println!("created {}", project::CONFIG_FILE);
                }
                println!("{REMINDERS}");
            }
            0
        }
        Err(report) => {
            eprintln!("{report:?}");
            EXIT_ERROR
        }
    }
}

const REMINDERS: &str = "Add a repo with `refs add <url>`, then run `refs sync`.
Linters, formatters and type checkers are yours to configure: exclude the references dir from them.
Claude Code reads AGENTS.md only when there is no CLAUDE.md; to use CLAUDE.md, list it in `agents_files`.";

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
