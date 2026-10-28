//! The command line: parsing and the entry points. What to do is decided by `sync`, `plan`, `add`
//! and `update`; this module parses the arguments into plain values and, in `dispatch`, strings the steps
//! in `commands` together for each command, which reports through `ui`.
//!
//! The `Source` is injected into `run` as a function of the loaded project (the real one
//! needs its root and `references_dir`): tests return the fake in-process (the `testing`
//! feature), and `main` builds a `GitSource`. There is no hidden flag, so the shipped binary
//! cannot be pointed at a fake.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::add::Asking;
use crate::commands;
use crate::edit::{AddRepo, Edit, Target};
use crate::registry::{self, Registry};
use crate::self_update::{self, Updater};
use crate::source::MakeSource;
use crate::sync::{self, SyncFlags};
use crate::ui::{self, Console, EXIT_ERROR, Prompter};
use crate::update;

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

/// The commands, by what they need: `init` and `self` run where there may be no project
/// (`self` is about the program, not a project), the config commands read and edit
/// `refs.toml` and ask for a `Source` only to sync after an edit (unless `--no-sync`) or to
/// show `list --status`, and the rest act on the project through one.
#[derive(Debug, Subcommand)]
enum Command {
    /// Set up a project: refs.toml, the managed block and the git exclude rule
    Init(InitArgs),
    /// Manage this program
    #[command(subcommand, name = "self")]
    SelfCommand(SelfCommand),
    /// Move repos to the version the project uses now, or to a ref given
    #[command(after_help = "\
A repo that cannot be assessed, or has uncommitted changes, is skipped and named; the rest \
are moved. A failure the remote causes after that, when the edited config is resolved and \
verified, blocks the whole run.")]
    Update(UpdateArgs),
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
        #[command(flatten)]
        input: Input,
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
    /// Discard local changes in a checkout the edit moves or removes
    #[arg(long, conflicts_with = "no_sync")]
    force: bool,
    /// Never contact the network
    #[arg(long)]
    offline: bool,
}

/// What the commands that can ask a question take.
#[derive(Debug, Args)]
struct Input {
    /// Never prompt, even in a terminal
    #[arg(long)]
    no_input: bool,
}

impl Input {
    /// `prompter`, unless this run was told never to ask.
    fn gate<'a>(&self, prompter: Option<&'a mut dyn Prompter>) -> Option<&'a mut dyn Prompter> {
        prompter.filter(|_| !self.no_input)
    }
}

impl EditFlags {
    /// What the sync an edit runs takes from the command line.
    fn sync_flags(&self) -> SyncFlags {
        SyncFlags {
            offline: self.offline,
            force: self.force,
            ..SyncFlags::default()
        }
    }
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
struct UpdateArgs {
    /// The repos to move; none opens a picker, or with no terminal lists
    #[arg(value_name = "ID")]
    ids: Vec<String>,
    /// Set the ref of the one repo named, without reading a lockfile
    #[arg(long = "ref", value_name = "REF")]
    git_ref: Option<String>,
    /// Move a repo whose checkout has uncommitted changes, discarding them
    #[arg(long)]
    force: bool,
    /// Print what would move and what it would break, and change nothing
    #[arg(long, conflicts_with = "force")]
    list: bool,
    #[command(flatten)]
    input: Input,
}

#[derive(Debug, Subcommand)]
enum SelfCommand {
    /// Replace this program with the latest release
    Update(SelfUpdateArgs),
}

#[derive(Debug, Args)]
struct SelfUpdateArgs {
    /// Install nothing; exit 3 if a newer release exists
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Args)]
struct RemoveArgs {
    /// The id of the repo to remove
    id: String,
    #[command(flatten)]
    flags: EditFlags,
}

#[derive(Debug, Args)]
struct SyncArgs {
    /// Change nothing; exit 3 if the lock, checkouts or blocks are out of date
    #[arg(long)]
    check: bool,
    /// Discard local changes in checkouts
    #[arg(long)]
    force: bool,
    /// Never contact the network
    #[arg(long)]
    offline: bool,
}

/// Run `refs` with `args` (the first is the program name) from `cwd`, and return the exit
/// code: 0 in sync, 1 error or refusal, 2 usage, 3 `--check` found drift. `make_source` is
/// given the project root and config once they are loaded.
pub fn run(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    make_source: impl MakeSource,
) -> u8 {
    use std::io::IsTerminal;
    let styled = no_color_env_unset();
    let styles = Styled {
        out: styled && std::io::stdout().is_terminal(),
        err: styled && std::io::stderr().is_terminal(),
    };
    // Prompts need someone to answer and somewhere to be drawn: stdin and stderr.
    let mut prompter = ui::Terminal;
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    run_on(
        args,
        cwd,
        make_source,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
        styles,
        Outside {
            prompter: interactive.then_some(&mut prompter as &mut dyn Prompter),
            updater: &self_update::Axo,
            registry: &registry::Http,
        },
    )
}

/// Which of the output streams are styled: a terminal, with `NO_COLOR` unset. Decided once
/// in `run`, so nothing below it reads the environment. Clap's messages and the dimming of
/// `list` follow it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Styled {
    pub out: bool,
    pub err: bool,
}

/// `run` with the two output streams given, so a test can read what went to each. Neither is
/// a terminal, so clap's messages are plain.
#[cfg(any(test, feature = "testing"))]
pub fn run_with(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    make_source: impl MakeSource,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    run_on(
        args,
        cwd,
        make_source,
        out,
        err,
        Styled::default(),
        Outside::offline(None),
    )
}

/// What a run reaches beyond its files and its `Source`, given so a test can script it.
pub struct Outside<'a> {
    /// Who to ask. `None` means there is nobody; `--no-input` (on `add` and `update`) ignores a given one.
    pub prompter: Option<&'a mut dyn Prompter>,
    /// What `self update` installs through.
    pub updater: &'a dyn Updater,
    /// Where `add npm:...` looks packages up.
    pub registry: &'a dyn Registry,
}

impl<'a> Outside<'a> {
    /// Nothing outside the run: no self update, no registry package. For tests that script
    /// neither.
    #[cfg(any(test, feature = "testing"))]
    pub fn offline(prompter: Option<&'a mut dyn Prompter>) -> Self {
        Outside {
            prompter,
            updater: &self_update::fake::Unmanaged,
            registry: &registry::fake::Unused,
        }
    }
}

/// `run_with`, told which streams are styled and given what the run reaches outside.
pub fn run_on(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    make_source: impl MakeSource,
    out: &mut dyn Write,
    err: &mut dyn Write,
    styles: Styled,
    outside: Outside,
) -> u8 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let styled = if e.use_stderr() {
                styles.err
            } else {
                styles.out
            };
            let rendered = e.render();
            let text = if styled {
                rendered.ansi().to_string()
            } else {
                rendered.to_string()
            };
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
    let color = !cli.no_color && styles.out;
    let mut console = Console::new(out, err, cli.quiet);
    dispatch(
        cli.command,
        start,
        make_source,
        color,
        outside,
        &mut console,
    )
    .unwrap_or_else(|code| code)
}

/// Run one command. `Err` is the exit code of a project or a `Source` that did not load; the
/// problems are already printed.
fn dispatch(
    command: Command,
    start: &Path,
    make_source: impl MakeSource,
    color: bool,
    outside: Outside,
    console: &mut Console,
) -> Result<u8, u8> {
    let code = match command {
        Command::Init(args) => commands::init(start, args.here, console),
        Command::SelfCommand(SelfCommand::Update(args)) => {
            commands::self_update(outside.updater, args.check, console)
        }
        Command::Update(args) => {
            let project = commands::load(start, console)?;
            let source = commands::source(make_source, &project, console)?;
            let request = update::Request {
                ids: &args.ids,
                git_ref: args.git_ref.as_deref(),
                force: args.force,
                list: args.list,
            };
            let prompter = args.input.gate(outside.prompter);
            commands::update(&*source, &project, &request, prompter, console)
        }
        Command::Config(ConfigCommand::List(args)) => {
            let project = commands::load(start, console)?;
            let source = if args.status {
                Some(commands::source(make_source, &project, console)?)
            } else {
                None
            };
            commands::list(&project, source.as_deref(), color, console)
        }
        Command::Config(ConfigCommand::Add { repo, flags, input }) => {
            // With no `refs.toml` yet, `add` makes one (ADR 0005).
            let project = commands::load_or_create(start, console)?;
            let source = if flags.no_sync {
                None
            } else {
                Some(commands::source(make_source, &project, console)?)
            };
            let asking = Asking {
                prompter: input.gate(outside.prompter),
                registry: outside.registry,
            };
            commands::add(
                source.as_deref(),
                &project,
                &repo,
                &flags.sync_flags(),
                asking,
                console,
            )
        }
        Command::Config(ConfigCommand::Remove(args)) => edit(
            start,
            make_source,
            Edit::Remove(&args.id),
            &args.flags,
            console,
        )?,
        Command::Config(ConfigCommand::Disable(args)) => edit(
            start,
            make_source,
            Edit::Disable(args.target()),
            &args.flags,
            console,
        )?,
        Command::Config(ConfigCommand::Enable(args)) => edit(
            start,
            make_source,
            Edit::Enable(args.target()),
            &args.flags,
            console,
        )?,
        Command::Source(SourceCommand::Sync(args)) => {
            let project = commands::load(start, console)?;
            let source = commands::source(make_source, &project, console)?;
            let flags = SyncFlags {
                offline: args.offline,
                force: args.force,
                check: args.check,
                ..SyncFlags::default()
            };
            let report = sync::sync(&*source, &project.root, &project.config, &flags);
            ui::print_report(&report, flags.check, console)
        }
    };
    Ok(code)
}

/// `remove`, `enable` and `disable`: an edit of `refs.toml`, then a sync unless `--no-sync`.
fn edit(
    start: &Path,
    make_source: impl MakeSource,
    edit: Edit,
    flags: &EditFlags,
    console: &mut Console,
) -> Result<u8, u8> {
    let project = commands::load(start, console)?;
    let report = if flags.no_sync {
        sync::edit_only(&project.root, &edit)
    } else {
        let source = commands::source(make_source, &project, console)?;
        sync::edit(&*source, &project.root, &edit, &flags.sync_flags())
    };
    Ok(ui::print_report(&report, false, console))
}

fn no_color_env_unset() -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
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
