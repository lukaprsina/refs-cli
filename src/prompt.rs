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

/// Asks one question and gives the answer. A prompter re-asks until `validate` accepts, and
/// gives `default` for an empty answer.
pub trait Prompter {
    /// A line of text. `Ok("")` means the person left it empty and there is no default.
    fn text(
        &mut self,
        message: &str,
        default: Option<&str>,
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort>;

    /// A line of text that may be one of `suggestions` (narrowed as the person types) or
    /// something new. The answer is not an empty string unless `validate` allows it.
    fn suggest(
        &mut self,
        message: &str,
        suggestions: &[String],
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort>;

    /// Yes or no, defaulting to no.
    fn confirm(&mut self, message: &str) -> Result<bool, Abort>;
}

/// A `Prompter` that remembers whether it was asked anything, so `add` can tell whether to
/// print the equivalent command line, whichever step asked.
pub struct Counting<'p> {
    inner: &'p mut dyn Prompter,
    asked: bool,
}

impl<'p> Counting<'p> {
    pub fn new(inner: &'p mut dyn Prompter) -> Self {
        Counting {
            inner,
            asked: false,
        }
    }

    /// Whether any question has been asked.
    pub fn asked(&self) -> bool {
        self.asked
    }
}

impl Prompter for Counting<'_> {
    fn text(
        &mut self,
        message: &str,
        default: Option<&str>,
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        self.asked = true;
        self.inner.text(message, default, validate)
    }

    fn suggest(
        &mut self,
        message: &str,
        suggestions: &[String],
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        self.asked = true;
        self.inner.suggest(message, suggestions, validate)
    }

    fn confirm(&mut self, message: &str) -> Result<bool, Abort> {
        self.asked = true;
        self.inner.confirm(message)
    }
}

/// Complete `repo` by asking for each value it does not have, except the URL, which `add` asks
/// first (`ask_url`) so that a shorthand is expanded before the rest. A value given on the command
/// line is never asked for again; an empty answer leaves a value absent, as omitting its flag
/// would.
///
/// The packages are not asked here: `add` reads them from the new Checkout and asks then (ADR
/// 0009), see `ask_packages`. With `ref_settled` the `ref` is not asked either: a registry add
/// has chosen it (a tag, or the default branch).
pub fn fill_add(
    repo: &mut AddRepo,
    config: &Config,
    prompter: &mut dyn Prompter,
    ref_settled: bool,
) -> Result<(), Abort> {
    if repo.id.is_none() {
        let default = id_from_url(&repo.url).to_lowercase();
        let taken = |id: &str| valid_id(id, config);
        let id = prompter.text("Id", Some(&default), &taken)?;
        repo.id = Some(id).filter(|id| *id != default);
    }
    if repo.group.is_none() {
        repo.group = ask_group(config, prompter)?;
    }
    if repo.git_ref.is_none() && !ref_settled {
        repo.git_ref = ask_optional(prompter, "Ref (empty: the remote's default branch)")?;
    }
    if repo.description.is_none() {
        repo.description = ask_optional(prompter, "Description (one line on what the repo is)")?;
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

/// The packages of a repo that has just been checked out, with `inferred` as the default. The
/// answer is split on whitespace; an empty one is none.
pub fn ask_packages(
    inferred: &[String],
    prompter: &mut dyn Prompter,
) -> Result<Vec<String>, Abort> {
    let default = inferred.join(" ");
    let default = Some(default.as_str()).filter(|d| !d.is_empty());
    let answer = prompter.text("Packages (space-separated)", default, &|_| Ok(()))?;
    Ok(answer.split_whitespace().map(str::to_owned).collect())
}

/// Whether to file the repo under a group, and which: one that exists or a new one, by name.
fn ask_group(config: &Config, prompter: &mut dyn Prompter) -> Result<Option<String>, Abort> {
    if !prompter.confirm("Link to a group?")? {
        return Ok(None);
    }
    let existing: Vec<String> = config.groups.keys().map(|id| id.as_ref().clone()).collect();
    let message = if existing.is_empty() {
        "Group (none yet; a new one will be created)"
    } else {
        "Group (type to filter, or a new name to create one)"
    };
    let name = prompter.suggest(message, &existing, &|name| valid_group(name, &existing))?;
    Ok(Some(name))
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
        return Err("enter a name, or press Esc to cancel".into());
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
        default: Option<&str>,
        validate: &dyn Fn(&str) -> Result<(), String>,
    ) -> Result<String, Abort> {
        let mut question = inquire::Text::new(message).with_validator(validator(validate));
        if let Some(default) = default {
            question = question.with_default(default);
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
