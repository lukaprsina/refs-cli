//! Interactive `add` (ADR 0008): ask for what the command line left out. The questions go
//! through the `Prompter` seam, so tests script the answers and only `cli::run` puts a
//! terminal behind it. Nothing here edits a file; it only completes an `AddRepo`.

use crate::config::{self, Config};
use crate::edit::{AddRepo, id_from_url};

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

    /// One of `options`, by index.
    fn select(&mut self, message: &str, options: &[String]) -> Result<usize, Abort>;

    /// Yes or no, defaulting to no.
    fn confirm(&mut self, message: &str) -> Result<bool, Abort>;
}

/// What `fill_add` found out: whether it asked anything at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filled {
    pub asked: bool,
}

const NO_GROUP: &str = "no group";
const NEW_GROUP: &str = "a new group...";

/// Complete `repo` by asking for each value it does not have. A value given on the command
/// line is never asked for again; an empty answer leaves a value absent, as omitting its flag
/// would.
pub fn fill_add(
    repo: &mut AddRepo,
    config: &Config,
    prompter: &mut dyn Prompter,
) -> Result<Filled, Abort> {
    let mut asked = false;
    if repo.url.is_empty() {
        asked = true;
        repo.url = prompter.text("Repository URL", None, &valid_url)?;
    }
    if repo.id.is_none() {
        asked = true;
        let default = id_from_url(&repo.url).to_lowercase();
        let taken = |id: &str| valid_id(id, config);
        let id = prompter.text("Id", Some(&default), &taken)?;
        repo.id = Some(id).filter(|id| *id != default);
    }
    if repo.group.is_none() && !config.groups.is_empty() {
        asked = true;
        repo.group = ask_group(config, prompter)?;
    }
    if repo.git_ref.is_none() {
        asked = true;
        repo.git_ref = ask_optional(prompter, "Ref (empty: the remote's default branch)")?;
    }
    if repo.description.is_none() {
        asked = true;
        repo.description = ask_optional(prompter, "Description (one line on what the repo is)")?;
    }
    let any_list_empty = repo.paths.is_empty() || repo.packages.is_empty() || repo.start.is_empty();
    if any_list_empty {
        asked = true;
        if prompter.confirm("Customize paths, packages and start?")? {
            for (list, message) in [
                (&mut repo.paths, "Paths to check out (space-separated)"),
                (
                    &mut repo.packages,
                    "Packages it documents (space-separated)",
                ),
                (&mut repo.start, "Files to read first (space-separated)"),
            ] {
                if list.is_empty() {
                    let answer = ask_optional(prompter, message)?.unwrap_or_default();
                    *list = answer.split_whitespace().map(str::to_owned).collect();
                }
            }
        }
    }
    Ok(Filled { asked })
}

fn ask_group(config: &Config, prompter: &mut dyn Prompter) -> Result<Option<String>, Abort> {
    let existing: Vec<String> = config.groups.keys().map(|id| id.as_ref().clone()).collect();
    let options: Vec<String> = std::iter::once(NO_GROUP.to_owned())
        .chain(existing.iter().cloned())
        .chain(std::iter::once(NEW_GROUP.to_owned()))
        .collect();
    let chosen = prompter.select("Group", &options)?;
    if chosen == 0 {
        Ok(None)
    } else if chosen == options.len() - 1 {
        let name = prompter.text("Name of the new group", None, &valid_group)?;
        Ok(non_empty(name))
    } else {
        Ok(Some(existing[chosen - 1].clone()))
    }
}

/// A free-text question that `config::parse` checks later; empty means absent.
fn ask_optional(prompter: &mut dyn Prompter, message: &str) -> Result<Option<String>, Abort> {
    Ok(non_empty(prompter.text(message, None, &|_| Ok(()))?))
}

fn non_empty(answer: String) -> Option<String> {
    Some(answer).filter(|a| !a.is_empty())
}

fn valid_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("a URL is required".into());
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

fn valid_group(name: &str) -> Result<(), String> {
    if name.is_empty() || config::is_heading_safe(name) {
        Ok(())
    } else {
        Err("a group name may not contain markup or control characters".into())
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
        let validator = |input: &str| {
            Ok(match validate(input) {
                Ok(()) => inquire::validator::Validation::Valid,
                Err(message) => inquire::validator::Validation::Invalid(message.into()),
            })
        };
        let mut question = inquire::Text::new(message).with_validator(validator);
        if let Some(default) = default {
            question = question.with_default(default);
        }
        question.prompt().map_err(abort)
    }

    fn select(&mut self, message: &str, options: &[String]) -> Result<usize, Abort> {
        inquire::Select::new(message, options.to_vec())
            .raw_prompt()
            .map(|chosen| chosen.index)
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
