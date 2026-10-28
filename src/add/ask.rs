//! The questions of `add` (ADR 0008): complete an `AddRepo` with what the command line left out.
//! Nothing here edits a file.

use super::Looked;
use crate::config::{self, Config};
use crate::edit::{AddRepo, id_from_url};
use crate::registry;
use crate::source::Pin;
use crate::ui::Say;
use crate::ui::pickers::{ask_optional, ask_packages, ask_paths, ask_start, non_empty};
use crate::ui::{Abort, Prompter};

/// Complete `repo` by showing a summary of what will be written and asking `Customize?`. A value
/// given on the command line is never asked for again; an empty answer leaves a value absent, as
/// omitting its flag would. The URL and the `ref` are not asked here: `add` asks them first, so
/// that a shorthand is expanded and the repo looked at before the rest (ADR 0011).
///
/// A value with no default is asked before the summary: an id that is taken, a description none
/// was found for. On yes, the rest are asked in turn, each starting from the summary's value, so
/// Enter all the way through gives what no gives. The `paths` and `start` are picked from the
/// looked-at repo's paths, else typed; narrowing `paths` recomputes the `packages` default.
pub(super) fn fill_add(
    repo: &mut AddRepo,
    config: &Config,
    prompter: &mut dyn Prompter,
    looked: &Looked,
    paths_from_registry: bool,
    say: &mut dyn Say,
) -> Result<(), Abort> {
    let suggested = id_from_url(&repo.url).to_lowercase();
    let mut id = repo.id.clone().unwrap_or_else(|| suggested.clone());
    let mut id_open = repo.id.is_none();
    if id_open && valid_id(&id, config).is_err() {
        id = ask_id(prompter, &id, config)?;
        id_open = false;
    }
    let mut description_open = repo.description.is_none();
    if description_open {
        repo.description = looked.described.clone();
    }
    if description_open && repo.description.is_none() {
        repo.description = ask_description(prompter, None)?;
        description_open = false;
    }
    let (paths_open, start_open, packages_open) = (
        repo.paths.is_empty() || paths_from_registry,
        repo.start.is_empty(),
        repo.packages.is_empty(),
    );
    let packages_for = |paths: &[String]| looked.manifests.packages_for(paths);
    if packages_open {
        repo.packages = packages_for(&repo.paths);
    }
    let summed = repo.packages.len();
    let group_open = repo.group.is_none();
    let open =
        id_open || description_open || group_open || paths_open || start_open || packages_open;
    if open {
        for line in summary(repo, &id, looked.outline.as_ref().map(|o| &o.pin)) {
            say.shown(line);
        }
    }
    if open && prompter.confirm("Customize?")? {
        let listed = looked.outline.as_ref().map(|o| &o.paths[..]);
        if id_open {
            id = ask_id(prompter, &id, config)?;
        }
        if group_open {
            repo.group = ask_group(config, prompter)?;
        }
        if description_open {
            let current = repo.description.clone();
            repo.description = ask_description(prompter, current.as_deref())?;
        }
        if paths_open {
            repo.paths = ask_paths(prompter, listed, (&repo.paths, &repo.paths), say)?;
            if packages_open {
                repo.packages = packages_for(&repo.paths);
            }
        }
        if start_open {
            repo.start = ask_start(prompter, listed, &repo.paths, (&[], &[]), say)?;
        }
        if packages_open && !repo.packages.is_empty() {
            if let Some(left) = summed.checked_sub(repo.packages.len()).filter(|n| *n > 0) {
                say.shown(format!(
                    "packages: left out {left} of {summed} that are outside the chosen paths; name others with `--packages`"
                ));
            }
            repo.packages = ask_packages(prompter, &repo.packages, &repo.packages)?;
        }
    }
    repo.id = Some(id).filter(|id| *id != suggested);
    Ok(())
}

/// The id to add the repo under, starting from `current`, until it is valid and not taken.
fn ask_id(prompter: &mut dyn Prompter, current: &str, config: &Config) -> Result<String, Abort> {
    prompter.text("Id", Some(current), &|id| valid_id(id, config))
}

/// The description, starting from `current`. Empty is none.
fn ask_description(
    prompter: &mut dyn Prompter,
    current: Option<&str>,
) -> Result<Option<String>, Abort> {
    let message = "Description (one line on what the repo is)";
    Ok(non_empty(prompter.text(message, current, &|_| Ok(()))?))
}

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

/// The repository URL, or a shorthand for one (`gh:owner/repo`, `npm:name`), which `add`
/// expands before `fill_add` asks the rest.
pub(super) fn ask_url(prompter: &mut dyn Prompter) -> Result<String, Abort> {
    prompter.text("Repository (URL or shorthand)", None, &valid_url)
}

/// The ref to add, asked once the URL is settled and before the repo is looked at. Empty is
/// the remote's default branch.
pub(super) fn ask_ref(prompter: &mut dyn Prompter) -> Result<Option<String>, Abort> {
    ask_optional(prompter, "Ref (empty: the remote's default branch)")
}

fn valid_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("a URL is required".into());
    }
    match registry::forge::name(url) {
        Some(Err(why)) => return Err(format!("`{url}` does not name a repository: {why}")),
        Some(Ok(_)) => return Ok(()),
        None if super::expand::is_shorthand(url) => return Ok(()),
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
pub(super) fn equivalent_command(repo: &AddRepo) -> String {
    let mut parts = vec!["refs".to_owned(), "add".to_owned(), quote(&repo.url)];
    for (flag, value) in [
        ("--id", &repo.id),
        ("--group", &repo.group),
        ("--ref", &repo.git_ref),
        ("--description", &repo.description),
        ("--origin", &repo.origin),
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
