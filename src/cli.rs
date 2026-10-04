//! The command line: parsing, output and exit codes. No logic (architecture.md): every
//! decision is made by `sync` and `plan`.
//!
//! The `Source` is injected into `run` as a function of the loaded project (the real one
//! needs its root and `references_dir`): tests return the fake in-process (the `testing`
//! feature), and `main` builds a `GitSource`. There is no hidden flag, so the shipped binary
//! cannot be pointed at a fake.

use std::ffi::OsString;
use std::fmt::Display;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::config::Config;
use crate::diagnostic::SourceError;
use crate::edit::{AddRepo, Edit, Target};
use crate::list::list;
use crate::plan::{LockFlags, Upgrade};
use crate::project;
use crate::source::Source;
use crate::sync::{self, Change, Changed, Checkout, Edited, Outcome, Report, SyncFlags};

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

/// Where a run's output goes, in uv's style: status lines on stderr (silenced by `-q`),
/// problems on stderr always, and stdout for data only.
struct Console<'w> {
    out: &'w mut dyn Write,
    err: &'w mut dyn Write,
    quiet: bool,
    /// How many status lines were asked for, shown or not: "nothing changed" is for a run
    /// that had none.
    said: usize,
}

impl Console<'_> {
    /// Data: what the command is for, such as the `list` table.
    fn data(&mut self, text: &str) {
        // A closed pipe is the reader's choice, not an error of ours.
        let _ = write!(self.out, "{text}");
    }

    /// A status line: what changed, or that nothing did.
    fn status(&mut self, line: impl Display) {
        self.said += 1;
        if !self.quiet {
            let _ = writeln!(self.err, "{line}");
        }
    }

    /// A problem, or a hint about one: always printed.
    fn problem(&mut self, line: impl Display) {
        let _ = writeln!(self.err, "{line}");
    }

    fn report(&mut self, report: impl std::fmt::Debug) {
        self.problem(format!("{report:?}"));
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
    run_with(
        args,
        cwd,
        make_source,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// `run` with the two output streams given, so a test can read what went to each.
pub fn run_with<'a>(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let text = e.render().to_string();
            let _ = if e.use_stderr() {
                write!(err, "{text}")
            } else {
                write!(out, "{text}")
            };
            return u8::try_from(e.exit_code()).unwrap_or(EXIT_ERROR);
        }
    };
    install_report_handler(cli.no_color);
    let start = cli.project.as_deref().unwrap_or(cwd);
    let color = !cli.no_color && stdout_is_colorful();
    let mut console = Console {
        out,
        err,
        quiet: cli.quiet,
        said: 0,
    };
    match cli.command {
        Command::Init(args) => init(start, args.here, &mut console),
        Command::Config(command) => match load(start, &mut console) {
            Ok((root, config)) => {
                run_config(&command, &root, &config, color, make_source, &mut console)
            }
            Err(code) => code,
        },
        Command::Source(command) => match load(start, &mut console) {
            Ok((root, config)) => run_source(&command, &root, &config, make_source, &mut console),
            Err(code) => code,
        },
    }
}

/// The project at or above `start`; on failure the problems are printed and the exit code
/// returned.
fn load(start: &Path, console: &mut Console) -> Result<(PathBuf, Config), u8> {
    project::load(start).map_err(|reports| {
        for report in reports {
            console.report(report);
        }
        EXIT_ERROR
    })
}

fn run_config<'a>(
    command: &ConfigCommand,
    root: &Path,
    config: &Config,
    color: bool,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    console: &mut Console,
) -> u8 {
    let (flags, edit) = match command {
        ConfigCommand::List(args) if args.status => {
            return list_status(root, config, color, make_source, console);
        }
        ConfigCommand::List(_) => {
            console.data(&list(config, color));
            return 0;
        }
        ConfigCommand::Add { repo, flags } => (flags, Edit::Add(repo)),
        ConfigCommand::Remove(args) => (&args.flags, Edit::Remove(&args.id)),
        ConfigCommand::Disable(args) => (&args.flags, Edit::Disable(args.target())),
        ConfigCommand::Enable(args) => (&args.flags, Edit::Enable(args.target())),
    };
    let edited = sync::edit(
        root,
        &edit,
        flags.no_sync,
        || make_source(root, config),
        &SyncFlags::default(),
    );
    print_edited(&edited, flags.no_sync, console)
}

/// `refs list --status`: the config, the Lock and what `inspect` finds for every repo.
fn list_status<'a>(
    root: &Path,
    config: &Config,
    color: bool,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    console: &mut Console,
) -> u8 {
    let result = make_source(root, config)
        .map_err(miette::Report::new)
        .and_then(|source| sync::status(&*source, root, config));
    match result {
        Ok(status) => {
            console.data(&crate::list::list_status(config, &status, color));
            0
        }
        Err(report) => fail(report, console),
    }
}

fn run_source<'a>(
    command: &SourceCommand,
    root: &Path,
    config: &Config,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    console: &mut Console,
) -> u8 {
    let source = match make_source(root, config) {
        Ok(source) => source,
        Err(e) => return fail(e, console),
    };
    let source = &*source;
    let (report, check, retry) = match command {
        SourceCommand::Lock(args) => {
            let flags = LockFlags {
                upgrade: args.upgrade(),
                offline: false,
            };
            (sync::lock(source, root, config, &flags), false, false)
        }
        SourceCommand::Sync(args) => {
            let flags = SyncFlags {
                offline: args.offline,
                force: args.force,
                check: args.check,
            };
            (sync::sync(source, root, config, &flags), args.check, true)
        }
    };
    print(&report, check, console);
    if retry && matches!(report.outcome, Outcome::Failed | Outcome::Refused) {
        console.problem(HINT_FIXED);
    }
    exit_code(report.outcome)
}

const HINT_SYNC: &str = "run `refs sync` to bring the project up to date";
const HINT_FIXED: &str = "run `refs sync` once the problem is fixed";

/// Say what `sync::edit` did, and give the exit code.
fn print_edited(edited: &Edited, no_sync: bool, console: &mut Console) -> u8 {
    if edited.change == Change::Written {
        console.status(format_args!("updated {}", project::CONFIG_FILE));
    }
    if let Some(group) = &edited.group_created {
        console.status(format_args!("created group `{group}`"));
    }
    print(&edited.report, false, console);
    let outcome = edited.report.outcome;
    if no_sync && edited.change == Change::Written {
        console.status(HINT_SYNC);
    }
    if matches!(outcome, Outcome::Failed | Outcome::Refused) {
        match edited.change {
            Change::Written => console.problem(format_args!(
                "{} was updated; {HINT_FIXED}",
                project::CONFIG_FILE
            )),
            Change::Rejected => {
                console.problem(format_args!("{} was not changed", project::CONFIG_FILE))
            }
            Change::Unchanged => console.problem(HINT_FIXED),
        }
    }
    exit_code(outcome)
}

/// `refs init`: it needs no loaded project, as there may be none yet.
fn init(start: &Path, here: bool, console: &mut Console) -> u8 {
    let result = project::init_root(start, here)
        .map_err(miette::Report::new)
        .and_then(|root| crate::init::init(&root).map_err(miette::Report::new));
    match result {
        Ok(done) => {
            if done.config_created {
                console.status(format_args!("created {}", project::CONFIG_FILE));
            }
            if console.said == 0 {
                console.status("nothing changed");
            } else {
                console.status(REMINDERS);
            }
            0
        }
        Err(report) => fail(report, console),
    }
}

const REMINDERS: &str = "Add a repo with `refs add <url>`.
Linters, formatters and type checkers are yours to configure: exclude the references dir from them.
Claude Code reads AGENTS.md only when there is no CLAUDE.md; to use CLAUDE.md, list it in `agents_files`.";

/// Print `error` as a report and give the exit code for a failure.
fn fail(error: impl Into<miette::Report>, console: &mut Console) -> u8 {
    console.report(error.into());
    EXIT_ERROR
}

fn exit_code(outcome: Outcome) -> u8 {
    match outcome {
        Outcome::InSync => 0,
        Outcome::OutOfDate => EXIT_OUT_OF_DATE,
        Outcome::Refused | Outcome::Failed => EXIT_ERROR,
    }
}

/// Dim only on a terminal, and not when `NO_COLOR` is set.
fn stdout_is_colorful() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
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

/// One status line per change, the problems, and "nothing changed" for a run that did no
/// change and had no problem (`--check` changes nothing by design: it says "up to date").
fn print(report: &Report, check: bool, console: &mut Console) {
    for change in &report.changes {
        match change {
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
        if console.quiet && report.outcome == Outcome::InSync {
            continue;
        }
        console.report(diagnostic);
    }
    if report.outcome == Outcome::OutOfDate {
        for drift in &report.drift {
            console.problem(format_args!("out of date: {drift}"));
        }
    }
    if report.outcome == Outcome::InSync && console.said == 0 {
        console.status(if check {
            "up to date"
        } else {
            "nothing changed"
        });
    }
}
