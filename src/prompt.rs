//! Interactive `add` (ADR 0008): ask for what the command line left out. The questions go
//! through the `Prompter` seam, so tests script the answers and only `cli::run` puts a
//! terminal behind it. Nothing here edits a file; it only completes an `AddRepo`.

use crate::config::{self, Config};
use crate::edit::{AddRepo, id_from_url};
use crate::registry;

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
}

/// Complete `repo` by asking for each value it does not have, except the URL and the `ref`,
/// which `add` asks first (`ask_url`, `ask_ref`) so that a shorthand is expanded, and the repo
/// looked at, before the rest. A value given on the command
/// line is never asked for again; an empty answer leaves a value absent, as omitting its flag
/// would.
///
/// The packages are not asked here: `add` reads them from the new Checkout and asks then (ADR
/// 0009), see `ask_packages`. The description question starts from `described`, the
/// registry's own.
pub fn fill_add(
    repo: &mut AddRepo,
    config: &Config,
    prompter: &mut dyn Prompter,
    described: Option<&str>,
) -> Result<(), Abort> {
    if repo.id.is_none() {
        let suggested = id_from_url(&repo.url).to_lowercase();
        let taken = |id: &str| valid_id(id, config);
        let id = prompter.text("Id", Some(&suggested), &taken)?;
        repo.id = Some(id).filter(|id| *id != suggested);
    }
    if repo.group.is_none() {
        repo.group = ask_group(config, prompter)?;
    }
    if repo.description.is_none() {
        let message = "Description (one line on what the repo is)";
        repo.description = non_empty(prompter.text(message, described, &|_| Ok(()))?);
    }
    if (repo.paths.is_empty() || repo.start.is_empty())
        && prompter.confirm("Customize paths and start?")?
    {
        for (list, message) in [
            (&mut repo.paths, "Paths to check out (space-separated)"),
            (&mut repo.start, "Files to read first (space-separated)"),
        ] {
            if list.is_empty() {
                let answer = ask_optional(prompter, message)?.unwrap_or_default();
                *list = answer.split_whitespace().map(str::to_owned).collect();
            }
        }
    }
    Ok(())
}

/// The packages of a repo that has just been checked out, with `inferred` to start from. The
/// answer is split on whitespace; an empty one is none.
pub fn ask_packages(
    inferred: &[String],
    prompter: &mut dyn Prompter,
) -> Result<Vec<String>, Abort> {
    let initial = inferred.join(" ");
    let initial = Some(initial.as_str()).filter(|d| !d.is_empty());
    let answer = prompter.text("Packages (space-separated)", initial, &|_| Ok(()))?;
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
