//! Interactive `add` (ADR 0008): ask for what the command line left out. The questions go
//! through the `Prompter` seam, so tests script the answers and only `cli::run` puts a
//! terminal behind it. Nothing here edits a file; it only completes an `AddRepo`.

use crate::config::{self, Config};
use crate::edit::{AddRepo, id_from_url};
use crate::listing;
use crate::registry;
use crate::registry::pin::Say;
use crate::source::{Outline, Pin};

/// Why a question went unanswered. Nothing has been edited yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Abort {
    /// The person gave up (Esc or Ctrl-C).
    Cancelled,
    /// The terminal failed.
    Failed(String),
}

/// Asks one question and gives the answer. A prompter re-asks until `validate` accepts.
pub trait Prompter {
    /// A line of text, which the person edits from `initial` when there is one. The answer is
    /// what is in the box: `Ok("")` means it is empty, and a person can clear a prefilled value.
    fn text(
        &mut self,
        message: &str,
        initial: Option<&str>,
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort>;

    /// A line of text that may be one of `suggestions` (narrowed as the person types) or
    /// something new. The answer may be empty if `validate` allows it.
    fn suggest(
        &mut self,
        message: &str,
        suggestions: &[String],
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort>;

    /// Yes or no, defaulting to no.
    fn confirm(&mut self, message: &str) -> Result<bool, Abort>;

    /// Any number of `options`, narrowed as the person types, with those in `selected` ticked
    /// to begin with. Selecting none is allowed: the message says what that means.
    fn pick(
        &mut self,
        message: &str,
        options: &[String],
        selected: &[String],
    ) -> Result<Vec<String>, Abort>;
}

/// The package names for a Checkout of the given `paths`.
pub type PackagesFor<'a> = dyn Fn(&[String]) -> Vec<String> + 'a;

/// What `add` knows about the repo before it asks about the rest.
#[derive(Default)]
pub struct Known<'a> {
    /// The description to start from: the registry's, else the root Manifest's.
    pub described: Option<&'a str>,
    /// What the repo was looked at for, when it was: the commit and the paths of its tree.
    pub looked_at: Option<&'a Outline>,
    /// Whether the `paths` of the repo are the registry's, to be picked from with those ticked,
    /// and not given on the command line.
    pub paths_from_registry: bool,
    /// The packages the Manifests give for a Checkout of these `paths`, when they were read.
    pub packages_for: Option<&'a PackagesFor<'a>>,
}

/// Complete `repo` by showing a summary of what will be written and asking `Customize?`. A value
/// given on the command line is never asked for again; an empty answer leaves a value absent, as
/// omitting its flag would. The URL and the `ref` are not asked here: `add` asks them first, so
/// that a shorthand is expanded and the repo looked at before the rest (ADR 0011).
///
/// A value with no default is asked before the summary: an id that is taken, a description none
/// was found for. On yes, the rest are asked in turn, each starting from the summary's value, so
/// Enter all the way through gives what no gives. The `paths` and `start` are picked from the
/// looked-at repo's paths, else typed; narrowing `paths` recomputes the `packages` default.
pub fn fill_add(
    repo: &mut AddRepo,
    config: &Config,
    prompter: &mut dyn Prompter,
    known: &Known,
    say: &mut dyn Say,
) -> Result<(), Abort> {
    let suggested = id_from_url(&repo.url).to_lowercase();
    let taken = |id: &str| valid_id(id, config);
    let mut id = repo.id.clone().unwrap_or_else(|| suggested.clone());
    let mut id_open = repo.id.is_none();
    if id_open && taken(&id).is_err() {
        id = prompter.text("Id", Some(&id), &taken)?;
        id_open = false;
    }
    let mut description_open = repo.description.is_none();
    if description_open {
        repo.description = known.described.map(str::to_owned);
    }
    if description_open && repo.description.is_none() {
        repo.description = non_empty(prompter.text(DESCRIPTION, None, &|_| Ok(()))?);
        description_open = false;
    }
    let (paths_open, start_open, packages_open) = (
        repo.paths.is_empty() || known.paths_from_registry,
        repo.start.is_empty(),
        repo.packages.is_empty(),
    );
    let packages_for = |paths: &[String]| known.packages_for.map_or_else(Vec::new, |f| f(paths));
    if packages_open {
        repo.packages = packages_for(&repo.paths);
    }
    let summed = repo.packages.len();
    let group_open = repo.group.is_none();
    let open =
        id_open || description_open || group_open || paths_open || start_open || packages_open;
    if open {
        for line in summary(repo, &id, known.looked_at.map(|o| &o.pin)) {
            say.shown(line);
        }
    }
    if open && prompter.confirm("Customize?")? {
        let listed = known.looked_at.map(|o| &o.paths[..]);
        if id_open {
            id = prompter.text("Id", Some(&id), &taken)?;
        }
        if group_open {
            repo.group = ask_group(config, prompter)?;
        }
        if description_open {
            let current = repo.description.clone();
            let answer = prompter.text(DESCRIPTION, current.as_deref(), &|_| Ok(()))?;
            repo.description = non_empty(answer);
        }
        if paths_open {
            repo.paths = ask_paths(prompter, listed, &repo.paths, say)?;
            if packages_open {
                repo.packages = packages_for(&repo.paths);
            }
        }
        if start_open {
            repo.start = ask_start(prompter, listed, &repo.paths, say)?;
        }
        if packages_open && !repo.packages.is_empty() {
            if let Some(left) = summed.checked_sub(repo.packages.len()).filter(|n| *n > 0) {
                say.shown(format!(
                    "packages: left out {left} of {summed} that are outside the chosen paths; name others with `--packages`"
                ));
            }
            let message = "Packages (type to filter; none selected: none)";
            repo.packages = prompter.pick(message, &repo.packages, &repo.packages)?;
        }
    }
    repo.id = Some(id).filter(|id| *id != suggested);
    Ok(())
}

const DESCRIPTION: &str = "Description (one line on what the repo is)";

/// The lines that say what `repo` (with this `id`) will be added as.
fn summary(repo: &AddRepo, id: &str, pin: Option<&Pin>) -> Vec<String> {
    let git_ref = match (pin, &repo.git_ref) {
        (Some(pin), _) => format!("{} ({})", pin.display_ref(), pin.short_id()),
        (None, Some(git_ref)) => git_ref.clone(),
        (None, None) => "(default branch)".to_owned(),
    };
    let list = |items: &[String], none: &str| match items {
        [] => none.to_owned(),
        _ => items.join(", "),
    };
    let text = |value: &Option<String>| value.clone().unwrap_or_else(|| "(none)".to_owned());
    [
        ("id", id.to_owned()),
        ("ref", git_ref),
        ("paths", list(&repo.paths, "(everything)")),
        ("start", list(&repo.start, "(none)")),
        ("packages", list(&repo.packages, "(none)")),
        ("description", text(&repo.description)),
        ("group", text(&repo.group)),
    ]
    .map(|(label, value)| format!("{label:<12}{value}"))
    .to_vec()
}

/// The directories to check out: picked from those of the `listed` repo with `current` ticked,
/// else typed.
fn ask_paths(
    prompter: &mut dyn Prompter,
    listed: Option<&[String]>,
    current: &[String],
    say: &mut dyn Say,
) -> Result<Vec<String>, Abort> {
    let Some(listed) = listed else {
        return ask_words(prompter, "Paths to check out (space-separated)");
    };
    let (dirs, left) = listing::directories(listed);
    let dirs = listing::with_ticked(dirs, current);
    let message = "Paths to check out (type to filter; none selected: the whole repo)";
    let picked = pick_capped(
        prompter,
        say,
        message,
        dirs,
        current,
        left,
        ("directories", "--paths"),
    )?;
    Ok(listing::drop_children(picked))
}

/// The files to read first: picked from the docs files the Checkout of `paths` will hold, else
/// typed.
fn ask_start(
    prompter: &mut dyn Prompter,
    listed: Option<&[String]>,
    paths: &[String],
    say: &mut dyn Say,
) -> Result<Vec<String>, Abort> {
    let Some(listed) = listed else {
        return ask_words(prompter, "Files to read first (space-separated)");
    };
    let (files, left) = listing::start_files(listed, paths);
    let message = "Files to read first (type to filter; none selected: none)";
    let flag = ("files", "--start");
    pick_capped(prompter, say, message, files, &[], left, flag)
}

/// A selection from `options`, with `ticked` ticked, of which `left` more were cut off: said, with
/// the `flag` that names them. Nothing is asked when there are none.
fn pick_capped(
    prompter: &mut dyn Prompter,
    say: &mut dyn Say,
    message: &str,
    options: Vec<String>,
    ticked: &[String],
    left: usize,
    (noun, flag): (&str, &str),
) -> Result<Vec<String>, Abort> {
    if options.is_empty() {
        return Ok(vec![]);
    }
    if left > 0 {
        let shown = options.len();
        say.status(format!(
            "listed {shown} of {} {noun}; name the others with `{flag}`",
            shown + left
        ));
    }
    prompter.pick(message, &options, ticked)
}

fn ask_words(prompter: &mut dyn Prompter, message: &str) -> Result<Vec<String>, Abort> {
    let answer = ask_optional(prompter, message)?.unwrap_or_default();
    Ok(answer.split_whitespace().map(str::to_owned).collect())
}

/// The group to file the repo under: one that exists or a new one, by name. Empty is none.
fn ask_group(config: &Config, prompter: &mut dyn Prompter) -> Result<Option<String>, Abort> {
    let existing: Vec<String> = config.groups.keys().map(|id| id.as_ref().clone()).collect();
    let message = if existing.is_empty() {
        "Group (none yet; Enter for none, or a name to create one)"
    } else {
        "Group (Enter for none; type to filter, or a new name to create one)"
    };
    let name = prompter.suggest(message, &existing, &|name| valid_group(name, &existing))?;
    Ok(non_empty(name))
}

/// A free-text question that `config::parse` checks later; empty means absent.
fn ask_optional(prompter: &mut dyn Prompter, message: &str) -> Result<Option<String>, Abort> {
    Ok(non_empty(prompter.text(message, None, &|_| Ok(()))?))
}

fn non_empty(answer: String) -> Option<String> {
    Some(answer).filter(|a| !a.is_empty())
}

/// The repository URL, or a shorthand for one (`gh:owner/repo`, `npm:name`), which `add`
/// expands before `fill_add` asks the rest.
pub fn ask_url(prompter: &mut dyn Prompter) -> Result<String, Abort> {
    prompter.text("Repository URL", None, &valid_url)
}

/// The ref to add, asked once the URL is settled and before the repo is looked at. Empty is
/// the remote's default branch.
pub fn ask_ref(prompter: &mut dyn Prompter) -> Result<Option<String>, Abort> {
    ask_optional(prompter, "Ref (empty: the remote's default branch)")
}

fn valid_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("a URL is required".into());
    }
    match registry::forge::name(url) {
        Some(Err(why)) => return Err(format!("`{url}` does not name a repository: {why}")),
        Some(Ok(_)) => return Ok(()),
        None if registry::is_shorthand(url) => return Ok(()),
        None => {}
    }
    match config::url_problem(url) {
        Some((problem, _)) => Err(problem.into()),
        None => Ok(()),
    }
}

fn valid_id(id: &str, config: &Config) -> Result<(), String> {
    if !config::is_repo_id(id) {
        return Err(
            "use lowercase letters, digits, `.`, `_` and `-`, starting with a letter or digit"
                .into(),
        );
    }
    if config.repos.contains_key(id) {
        return Err(format!("`{id}` is already a repo"));
    }
    Ok(())
}

fn valid_group(name: &str, existing: &[String]) -> Result<(), String> {
    if name.is_empty() {
        return Ok(());
    }
    if !config::is_heading_safe(name) {
        return Err("a group name may not contain markup or control characters".into());
    }
    match existing
        .iter()
        .find(|group| group.to_lowercase() == name.to_lowercase())
    {
        Some(group) if group != name => Err(format!("did you mean `{group}`?")),
        _ => Ok(()),
    }
}

/// The command line that gives `repo` without any prompt, for the person to learn the flags
/// from.
pub fn equivalent_command(repo: &AddRepo) -> String {
    let mut parts = vec!["refs".to_owned(), "add".to_owned(), quote(&repo.url)];
    for (flag, value) in [
        ("--id", &repo.id),
        ("--group", &repo.group),
        ("--ref", &repo.git_ref),
        ("--description", &repo.description),
    ] {
        if let Some(value) = value {
            parts.push(flag.to_owned());
            parts.push(quote(value));
        }
    }
    for (flag, list) in [
        ("--paths", &repo.paths),
        ("--packages", &repo.packages),
        ("--start", &repo.start),
    ] {
        if !list.is_empty() {
            parts.push(flag.to_owned());
            parts.extend(list.iter().map(|item| quote(item)));
        }
    }
    parts.join(" ")
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@=+,".contains(c));
    if plain {
        word.to_owned()
    } else {
        format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// The real terminal, through `inquire`.
pub struct Terminal;

impl Prompter for Terminal {
    fn text(
        &mut self,
        message: &str,
        initial: Option<&str>,
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        let mut question = inquire::Text::new(message).with_validator(validator(validate));
        if let Some(initial) = initial {
            question = question.with_initial_value(initial);
        }
        question.prompt().map_err(abort)
    }

    fn suggest(
        &mut self,
        message: &str,
        suggestions: &[String],
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        inquire::Text::new(message)
            .with_autocomplete(Suggestions(suggestions.to_vec()))
            .with_validator(validator(validate))
            .prompt()
            .map_err(abort)
    }

    fn confirm(&mut self, message: &str) -> Result<bool, Abort> {
        inquire::Confirm::new(message)
            .with_default(false)
            .prompt()
            .map_err(abort)
    }

    fn pick(
        &mut self,
        message: &str,
        options: &[String],
        selected: &[String],
    ) -> Result<Vec<String>, Abort> {
        let ticked: Vec<usize> = (0..options.len())
            .filter(|&i| selected.contains(&options[i]))
            .collect();
        inquire::MultiSelect::new(message, options.to_vec())
            .with_default(&ticked)
            .with_page_size(15)
            .with_scorer(&path_scorer())
            .prompt()
            .map_err(abort)
    }
}

/// Scores an option (a path) against what has been typed, for `inquire`: fuzzy, with the bonuses
/// of path matching. `inquire` sorts by score and leaves ties in no order, so the option's
/// position, which is path order, breaks them: `packages/solid` stays above
/// `packages/solid-element`.
pub fn path_scorer() -> impl Fn(&str, &String, &str, usize) -> Option<i64> {
    use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
    use nucleo_matcher::{Config, Matcher, Utf32Str};
    let state = std::cell::RefCell::new((Matcher::new(Config::DEFAULT.match_paths()), Vec::new()));
    move |input, _, path, position| {
        let (matcher, buffer) = &mut *state.borrow_mut();
        let pattern = Pattern::new(
            input,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let score = pattern.score(Utf32Str::new(path, buffer), matcher)?;
        Some(i64::from(score) * 1_000_000 - position as i64)
    }
}

fn abort(error: inquire::InquireError) -> Abort {
    use inquire::InquireError::{OperationCanceled, OperationInterrupted};
    match error {
        OperationCanceled | OperationInterrupted => Abort::Cancelled,
        other => Abort::Failed(other.to_string()),
    }
}

/// `validate` as an `inquire` validator.
fn validator<'v>(
    validate: &'v dyn Fn(&str) -> Result<(), String>,
) -> impl inquire::validator::StringValidator + 'v {
    move |input: &str| {
        Ok(match validate(input) {
            Ok(()) => inquire::validator::Validation::Valid,
            Err(message) => inquire::validator::Validation::Invalid(message.into()),
        })
    }
}

/// The existing names, filtered by what has been typed; tab takes the highlighted one.
#[derive(Clone)]
struct Suggestions(Vec<String>);

impl inquire::Autocomplete for Suggestions {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, inquire::CustomUserError> {
        let input = input.to_lowercase();
        Ok(self
            .0
            .iter()
            .filter(|name| name.to_lowercase().contains(&input))
            .cloned()
            .collect())
    }

    fn get_completion(
        &mut self,
        _input: &str,
        highlighted: Option<String>,
    ) -> Result<inquire::autocompletion::Replacement, inquire::CustomUserError> {
        Ok(highlighted)
    }
}
