//! The command line: parsing, output and exit codes. No logic: every
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

use crate::add::Skipped;
use crate::config::Config;
use crate::diagnostic::SourceError;
use crate::edit::{AddRepo, Edit, Target};
use crate::list::list;
use crate::plan::{LockFlags, Upgrade};
use crate::project;
use crate::prompt::{self, Packages, Prompter};
use crate::registry::tag::tag_for;
use crate::registry::used::{self, Lookup, Pick};
use crate::registry::{self, Registry, Release};
use crate::source::Source;
use crate::sync::{self, Changed, Checkout, Hint, Outcome, Report, SyncFlags};
use crate::update::{self, Updater};

const EXIT_ERROR: u8 = 1;
const EXIT_USAGE: u8 = 2;
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
    /// Never prompt, even in a terminal
    #[arg(long, global = true)]
    no_input: bool,
    /// Never contact the network
    #[arg(long, global = true)]
    offline: bool,
    #[command(subcommand)]
    command: Command,
}

/// The commands, by what they need: `init` and `update` run where there may be no project
/// (`update` is about the program, not a project), the config commands read and edit
/// `refs.toml` and ask for a `Source` only to sync after an edit (unless `--no-sync`) or to
/// show `list --status`, and the rest act on the project through one.
#[derive(Debug, Subcommand)]
enum Command {
    /// Set up a project: refs.toml, the managed block and the git exclude rule
    Init(InitArgs),
    /// Replace this program with the latest release
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
struct UpdateArgs {
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
}

impl Console<'_> {
    /// Data: what the command is for, such as the `list` table.
    fn data(&mut self, text: &str) {
        // A closed pipe is the reader's choice, not an error of ours.
        let _ = write!(self.out, "{text}");
    }

    /// A status line: what changed, or that nothing did.
    fn status(&mut self, line: impl Display) {
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
    use std::io::IsTerminal;
    let styled = no_color_env_unset();
    let terminal = Terminal {
        out: styled && std::io::stdout().is_terminal(),
        err: styled && std::io::stderr().is_terminal(),
    };
    // Prompts need someone to answer and somewhere to be drawn: stdin and stderr.
    let mut prompter = prompt::Terminal;
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    run_on(
        args,
        cwd,
        make_source,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
        terminal,
        Outside {
            prompter: interactive.then_some(&mut prompter as &mut dyn Prompter),
            updater: &update::Axo,
            registry: &registry::Http::new(registry::Reqwest),
        },
    )
}

/// Which of the output streams are styled: a terminal, with `NO_COLOR` unset. Decided once
/// in `run`, so nothing below it reads the environment. Clap's messages and the dimming of
/// `list` follow it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Terminal {
    pub out: bool,
    pub err: bool,
}

/// `run` with the two output streams given, so a test can read what went to each. Neither is
/// a terminal, so clap's messages are plain.
pub fn run_with<'a>(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    run_on(
        args,
        cwd,
        make_source,
        out,
        err,
        Terminal::default(),
        Outside {
            prompter: None,
            updater: &update::Unmanaged,
            registry: &registry::Unavailable,
        },
    )
}

/// What a run reaches beyond its files and its `Source`, given so a test can script it.
pub struct Outside<'a> {
    /// Who to ask. `None` means there is nobody; `--no-input` ignores a given prompter.
    pub prompter: Option<&'a mut dyn Prompter>,
    /// What `update` installs through.
    pub updater: &'a dyn Updater,
    /// Where `add npm:...` looks packages up.
    pub registry: &'a dyn Registry,
}

/// `run_with`, told which streams are styled and given what the run reaches outside.
pub fn run_on<'a>(
    args: impl IntoIterator<Item = OsString>,
    cwd: &Path,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    terminal: Terminal,
    outside: Outside,
) -> u8 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let styled = if e.use_stderr() {
                terminal.err
            } else {
                terminal.out
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
    let color = !cli.no_color && terminal.out;
    let mut console = Console {
        out,
        err,
        quiet: cli.quiet,
    };
    match cli.command {
        Command::Init(args) => init(start, args.here, &mut console),
        Command::Update(args) => update(outside.updater, args.check, &mut console),
        Command::Config(command) => match load(start, &mut console) {
            Ok((root, config)) => {
                let prompter = outside.prompter.filter(|_| !cli.no_input);
                run_config(
                    &command,
                    &root,
                    &config,
                    color,
                    make_source,
                    Asking {
                        prompter,
                        registry: outside.registry,
                        offline: cli.offline,
                    },
                    &mut console,
                )
            }
            Err(code) => code,
        },
        Command::Source(command) => match load(start, &mut console) {
            Ok((root, config)) => run_source(
                &command,
                &root,
                &config,
                cli.offline,
                make_source,
                &mut console,
            ),
            Err(code) => code,
        },
    }
}

/// The project at or above `start`; on failure the problems are printed and the exit code
/// returned.
fn load(start: &Path, console: &mut Console) -> Result<(PathBuf, Config), u8> {
    project::load(start).map_err(|reports| fail_all(reports, console))
}

/// Who and what a config command may ask: a person, a registry, unless `--offline`.
struct Asking<'a> {
    prompter: Option<&'a mut dyn Prompter>,
    registry: &'a dyn Registry,
    offline: bool,
}

fn run_config<'a>(
    command: &ConfigCommand,
    root: &Path,
    config: &Config,
    color: bool,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    mut asking: Asking,
    console: &mut Console,
) -> u8 {
    let flags_for_sync = SyncFlags {
        offline: asking.offline,
        ..SyncFlags::default()
    };
    let (flags, edit) = match command {
        ConfigCommand::List(args) if args.status => {
            return with_source(make_source, root, config, console, |source, console| {
                list_status(source, root, config, color, console)
            });
        }
        ConfigCommand::List(_) => {
            console.data(&list(config, color));
            return 0;
        }
        ConfigCommand::Add { repo, flags } if flags.no_sync => {
            let (repo, _) = match prepare_add(repo, None, root, config, &mut asking, console) {
                Ok(prepared) => prepared,
                Err(code) => return code,
            };
            return print_report(&sync::edit_only(root, &Edit::Add(&repo)), false, console);
        }
        ConfigCommand::Add { repo, .. } => {
            return with_source(make_source, root, config, console, |source, console| {
                let (repo, asked) =
                    match prepare_add(repo, Some(source), root, config, &mut asking, console) {
                        Ok(prepared) => prepared,
                        Err(code) => return code,
                    };
                let job = AddJob {
                    repo: &repo,
                    flags: &flags_for_sync,
                    asked,
                };
                add(source, root, config, &job, asking.prompter, console)
            });
        }
        ConfigCommand::Remove(args) => (&args.flags, Edit::Remove(&args.id)),
        ConfigCommand::Disable(args) => (&args.flags, Edit::Disable(args.target())),
        ConfigCommand::Enable(args) => (&args.flags, Edit::Enable(args.target())),
    };
    if flags.no_sync {
        print_report(&sync::edit_only(root, &edit), false, console)
    } else {
        with_source(make_source, root, config, console, |source, console| {
            let report = sync::edit(source, root, &edit, &flags_for_sync);
            print_report(&report, false, console)
        })
    }
}

/// `repo` ready to add: a registry shorthand expanded and its version pinned to a tag, then
/// completed (see `complete_add`). The version is the one given, else the one the project's
/// Package lockfile at `root` says it uses. With a `source` a sync follows, so the `packages`
/// are left to `add`; without one (`--no-sync`) nothing can be listed or read.
fn prepare_add(
    repo: &AddRepo,
    source: Option<&dyn Source>,
    root: &Path,
    config: &Config,
    asking: &mut Asking,
    console: &mut Console,
) -> Result<(AddRepo, bool), u8> {
    let expanded = match registry::expand(repo, asking.registry, asking.offline) {
        Ok(expanded) => expanded,
        Err(e) => return Err(fail(e, console)),
    };
    let mut repo = expanded.repo;
    // An explicit `ref` wins over any version.
    if let (None, Some(release)) = (&repo.git_ref, &expanded.release) {
        let version = match &release.version {
            Some(version) => Some(version.clone()),
            None => used_version(release, root, asking.prompter.as_deref_mut(), console)?,
        };
        if let Some(version) = version {
            pin_release(
                &mut repo,
                &release.name,
                &version,
                source,
                asking.prompter.as_deref_mut(),
                console,
            )?;
        }
    }
    let packages = if source.is_some() {
        Packages::Later
    } else {
        Packages::Now
    };
    complete_add(
        &repo,
        config,
        asking.prompter.as_deref_mut(),
        packages,
        console,
    )
}

/// The version of `release` that the Package lockfile of the project at `root` says it uses,
/// and what was read is said. `None` when there is none to use: the repo then follows the
/// remote's default branch, and the reason is said. Several versions with nothing to choose
/// between them are asked about, or refused without a terminal. Never a guess.
fn used_version(
    release: &Release,
    root: &Path,
    prompter: Option<&mut (dyn Prompter + '_)>,
    console: &mut Console,
) -> Result<Option<String>, u8> {
    let name = &release.name;
    if !used::reads(release.ecosystem) {
        return Ok(None);
    }
    let follows = "the repo follows the remote's default branch";
    let found = match used::find(release.ecosystem, name, root) {
        Lookup::Found(found) => found,
        Lookup::Missing => {
            console.status(format_args!(
                "no Package lockfile found for `{name}`; {follows}"
            ));
            return Ok(None);
        }
        Lookup::Unreadable { file, why } => {
            console.problem(format_args!(
                "warning: cannot read {}: {why}; {follows}",
                file_name(&file, root)
            ));
            return Ok(None);
        }
    };
    let file = file_name(&found.file, root);
    for ignored in &found.ignored {
        console.status(format_args!(
            "{} is also here and was not read; {file} was",
            file_name(ignored, root)
        ));
    }
    match found.pick() {
        Pick::Nothing => {
            console.status(format_args!("`{name}` is not in {file}; {follows}"));
            Ok(None)
        }
        Pick::One(version) => {
            console.status(format_args!("`{name}` is {version} in {file}"));
            Ok(Some(version.to_owned()))
        }
        Pick::Several(versions) => {
            let list = versions.join(", ");
            let Some(prompter) = prompter else {
                console.problem(format_args!(
                    "error: {file} has several versions of `{name}` ({list}), and none is the \
                     one the project asks for; give one as `{name}@version`"
                ));
                return Err(EXIT_ERROR);
            };
            let highest = versions.last().copied();
            let one_of_them = |answer: &str| match versions.contains(&answer) {
                true => Ok(()),
                false => Err(format!("one of {list}")),
            };
            let message = format!("{file} has several versions of `{name}`: {list}. Which one?");
            match prompter.text(&message, highest, &one_of_them) {
                Ok(version) => Ok(Some(version)),
                Err(abort) => Err(aborted(abort, console)),
            }
        }
    }
}

/// `file` as it is named in a message: relative to the project `root` if it is below it.
fn file_name(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string()
}

/// Say why a question ended without an answer, and give the exit code.
fn aborted(abort: prompt::Abort, console: &mut Console) -> u8 {
    match abort {
        prompt::Abort::Cancelled => console.problem("cancelled"),
        prompt::Abort::Failed(why) => console.problem(format_args!("cannot prompt: {why}")),
    }
    EXIT_ERROR
}

/// Set the `ref` of `repo` to the tag that `name` at `version` was released as. A version with
/// no tag is never pinned silently: with a prompter the person is asked whether to follow the
/// remote's default branch instead, otherwise it is a warning and the repo follows it.
fn pin_release(
    repo: &mut AddRepo,
    name: &str,
    version: &str,
    source: Option<&dyn Source>,
    prompter: Option<&mut (dyn Prompter + '_)>,
    console: &mut Console,
) -> Result<(), u8> {
    let Some(source) = source else {
        console.problem(format_args!(
            "warning: `{name}@{version}` is not pinned to a tag, as `--no-sync` lists no tags; \
             the repo follows the remote's default branch"
        ));
        return Ok(());
    };
    let tags = source.tags(&repo.url).map_err(|e| fail(e, console))?;
    if let Some(tag) = tag_for(name, version, &tags) {
        console.status(format_args!("pinned `{name}@{version}` to the tag `{tag}`"));
        repo.git_ref = Some(tag.to_owned());
        return Ok(());
    }
    let untagged = format!("{name}@{version} is not tagged in {}", repo.url);
    let Some(prompter) = prompter else {
        console.problem(format_args!(
            "warning: {untagged}; the repo follows the remote's default branch"
        ));
        return Ok(());
    };
    match prompter.confirm(&format!(
        "{untagged}. Follow the remote's default branch instead?"
    )) {
        Ok(true) => Ok(()),
        Ok(false) => Err(aborted(prompt::Abort::Cancelled, console)),
        Err(abort) => Err(aborted(abort, console)),
    }
}

/// `repo` with what the command line left out, and whether anything was asked: asked for when
/// there is a prompter, otherwise only a missing URL is a problem (usage, exit 2). On a cancel
/// nothing has been edited. The `equivalent:` line is printed here unless `Packages::Later`,
/// when `add` prints it once the packages are known.
fn complete_add(
    repo: &AddRepo,
    config: &Config,
    prompter: Option<&mut (dyn Prompter + '_)>,
    packages: Packages,
    console: &mut Console,
) -> Result<(AddRepo, bool), u8> {
    let mut repo = repo.clone();
    let Some(prompter) = prompter else {
        if repo.url.is_empty() {
            console.problem("error: a URL is required when not run in a terminal");
            return Err(EXIT_USAGE);
        }
        return Ok((repo, false));
    };
    match prompt::fill_add(&mut repo, config, prompter, packages) {
        Ok(filled) => {
            if filled.asked && packages == Packages::Now {
                equivalent(&repo, console);
            }
            Ok((repo, filled.asked))
        }
        Err(abort) => Err(aborted(abort, console)),
    }
}

fn equivalent(repo: &AddRepo, console: &mut Console) {
    console.status(format_args!(
        "equivalent: {}",
        prompt::equivalent_command(repo)
    ));
}

/// The repo an `add` was asked for, how to sync it, and whether anything was asked on the way.
struct AddJob<'r> {
    repo: &'r AddRepo,
    flags: &'r SyncFlags,
    asked: bool,
}

/// `refs add` with a sync: the add, then the `packages` read from the new Checkout. When
/// something was `asked`, the `equivalent:` line comes last, with those `packages`.
fn add(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    job: &AddJob,
    prompter: Option<&mut dyn Prompter>,
    console: &mut Console,
) -> u8 {
    let AddJob { repo, flags, asked } = *job;
    let added = crate::add::run(source, root, config, repo, flags, prompter);
    let mut code = 0;
    for report in &added.reports {
        let reported = print_report(report, false, console);
        if code == 0 {
            code = reported;
        }
    }
    if asked {
        equivalent(&added.repo, console);
    }
    if let Some(why) = &added.skipped {
        if let Skipped::PromptFailed(why) = why {
            console.problem(format_args!("cannot prompt: {why}"));
        }
        console.status(format_args!(
            "added `{}` without packages; set them in {}",
            repo.resolved_id(),
            project::CONFIG_FILE
        ));
    }
    code
}

/// Build the `Source` and run `f` with it. Only the commands that sync ask for one, so the
/// others do not need a cache directory.
fn with_source<'a>(
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    root: &Path,
    config: &Config,
    console: &mut Console,
    f: impl FnOnce(&dyn Source, &mut Console) -> u8,
) -> u8 {
    match make_source(root, config) {
        Ok(source) => f(&*source, console),
        Err(e) => fail(e, console),
    }
}

/// `refs list --status`: the config, the Lock and the Checkout state of each active repo.
fn list_status(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    color: bool,
    console: &mut Console,
) -> u8 {
    match crate::status::build(source, root, config) {
        Ok(status) => {
            console.data(&crate::list::list_status(
                config,
                |laid| status.row(laid),
                color,
            ));
            0
        }
        Err(reports) => fail_all(reports, console),
    }
}

fn run_source<'a>(
    command: &SourceCommand,
    root: &Path,
    config: &Config,
    offline: bool,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    console: &mut Console,
) -> u8 {
    with_source(make_source, root, config, console, |source, console| {
        let (report, check) = match command {
            SourceCommand::Lock(args) => {
                let flags = LockFlags {
                    upgrade: args.upgrade(),
                    offline,
                };
                (sync::lock(source, root, config, &flags), false)
            }
            SourceCommand::Sync(args) => {
                let flags = SyncFlags {
                    offline,
                    force: args.force,
                    check: args.check,
                };
                (sync::sync(source, root, config, &flags), args.check)
            }
        };
        print_report(&report, check, console)
    })
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
        Hint::ConfigUnchangedTryNoSync => console.problem(format_args!(
            "{} was not changed; fix the broken repo, or pass `--no-sync` to edit without syncing",
            project::CONFIG_FILE
        )),
    }
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
                console.status(REMINDERS);
            } else {
                console.status("nothing changed");
            }
            0
        }
        Err(report) => fail(report, console),
    }
}

/// `refs update`: install the latest release over this one, or with `--check` only say
/// whether there is one (exit 3, as `sync --check` does for a project that is out of date).
fn update(updater: &dyn Updater, check: bool, console: &mut Console) -> u8 {
    if check {
        return match updater.is_update_available() {
            Ok(true) => {
                console.problem("out of date: a newer release is available; run `refs update`");
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

const REMINDERS: &str = "Add a repo with `refs add <url>`.
Linters, formatters and type checkers are yours to configure: exclude the references dir from them.
Claude Code reads AGENTS.md only when there is no CLAUDE.md; to use CLAUDE.md, list it in `agents_files`.";

/// Print `error` as a report and give the exit code for a failure.
fn fail(error: impl Into<miette::Report>, console: &mut Console) -> u8 {
    fail_all([error.into()], console)
}

/// Print every report and give the exit code for a failure.
fn fail_all(reports: impl IntoIterator<Item = miette::Report>, console: &mut Console) -> u8 {
    for report in reports {
        console.report(report);
    }
    EXIT_ERROR
}

fn exit_code(outcome: Outcome) -> u8 {
    match outcome {
        Outcome::InSync => 0,
        Outcome::OutOfDate => EXIT_OUT_OF_DATE,
        Outcome::Refused | Outcome::Failed => EXIT_ERROR,
    }
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

/// One status line per change, the problems, and "nothing changed" for a run that did no
/// change and had no problem (`--check` changes nothing by design: it says "up to date").
/// Gives the exit code.
fn print_report(report: &Report, check: bool, console: &mut Console) -> u8 {
    for change in &report.changes {
        match change {
            Changed::Config => console.status(format_args!("updated {}", project::CONFIG_FILE)),
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
