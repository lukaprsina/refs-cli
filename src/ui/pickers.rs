//! The pickers `add` and `update` share: the questions about `paths`, `start`, `packages` and
//! which of several Used versions, and the free-text helpers under them.

use crate::outline::{self, Outline};
use crate::ui::{Abort, Prompter, Say};

/// What a picker offers beyond what the tree lists, and which of it starts ticked. A ticked
/// entry is always offered.
pub(crate) type Entries<'a> = (&'a [String], &'a [String]);

/// The directories to check out: picked from those of the `outline`d repo, else typed. Of the
/// `(offered, ticked)` entries the first are offered even if the tree lacks them, and the
/// second start ticked.
pub(crate) fn ask_paths(
    prompter: &mut dyn Prompter,
    outline: Option<&Outline>,
    (offered, ticked): Entries,
    say: &mut dyn Say,
) -> Result<Vec<String>, Abort> {
    let Some(outline) = outline else {
        return ask_words(prompter, "Paths to check out (space-separated)");
    };
    let (dirs, left) = outline.directories();
    let dirs = outline::with_ticked(dirs, offered);
    let message = "Paths to check out (type to filter; none selected: the whole repo)";
    let picked = pick_capped(
        prompter,
        say,
        message,
        dirs,
        ticked,
        left,
        ("directories", "--paths"),
    )?;
    Ok(outline::drop_children(picked))
}

/// The files to read first: picked from the docs files the Checkout of `paths` will hold, else
/// typed. `(offered, ticked)` are as for `ask_paths`.
pub(crate) fn ask_start(
    prompter: &mut dyn Prompter,
    outline: Option<&Outline>,
    paths: &[String],
    (offered, ticked): Entries,
    say: &mut dyn Say,
) -> Result<Vec<String>, Abort> {
    let Some(outline) = outline else {
        return ask_words(prompter, "Files to read first (space-separated)");
    };
    let (files, left) = outline.start_files(paths);
    let files = outline::with_ticked(files, offered);
    let message = "Files to read first (type to filter; none selected: none)";
    let flag = ("files", "--start");
    pick_capped(prompter, say, message, files, ticked, left, flag)
}

/// The packages to document: picked from `offered`, with `ticked` ticked.
pub(crate) fn ask_packages(
    prompter: &mut dyn Prompter,
    offered: &[String],
    ticked: &[String],
) -> Result<Vec<String>, Abort> {
    let message = "Packages (type to filter; none selected: none)";
    prompter.pick(message, offered, ticked)
}

/// Which of `versions` of `name`, all in the Package lockfile `file`, to use; the highest is the
/// default.
pub(crate) fn ask_version(
    prompter: &mut dyn Prompter,
    name: &str,
    versions: &[String],
    file: &str,
) -> Result<String, Abort> {
    let one_of_them = |answer: &str| match versions.iter().any(|v| v == answer) {
        true => Ok(()),
        false => Err(format!("one of {}", versions.join(", "))),
    };
    let message = format!(
        "{file} has several versions of `{name}`: {}. Which one?",
        versions.join(", ")
    );
    prompter.text(&message, versions.last().map(String::as_str), &one_of_them)
}

/// Why `versions` of `name` in `file` cannot be chosen between with no one to ask: the start of
/// the refusal, which the caller ends with what to do instead.
pub(crate) fn several_versions(name: &str, versions: &[String], file: &str) -> String {
    format!(
        "{file} has several versions of `{name}` ({}), and none is the one the project asks for",
        versions.join(", ")
    )
}

/// A selection from `options`, with `ticked` ticked, of which `left` more were cut off: said, with
/// the `flag` that names them. Nothing is asked when there are none.
pub(crate) fn pick_capped(
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

/// Space-separated words, for when there is no tree to pick from. Empty is none.
pub(crate) fn ask_words(prompter: &mut dyn Prompter, message: &str) -> Result<Vec<String>, Abort> {
    let answer = ask_optional(prompter, message)?.unwrap_or_default();
    Ok(answer.split_whitespace().map(str::to_owned).collect())
}

/// A free-text question that `config::parse` checks later; empty means absent.
pub(crate) fn ask_optional(
    prompter: &mut dyn Prompter,
    message: &str,
) -> Result<Option<String>, Abort> {
    Ok(non_empty(prompter.text(message, None, &|_| Ok(()))?))
}

/// `None` for an empty answer.
pub(crate) fn non_empty(answer: String) -> Option<String> {
    Some(answer).filter(|a| !a.is_empty())
}
