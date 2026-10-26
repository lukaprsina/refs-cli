//! The real terminal: the `inquire` adapter of `Prompter`, and `path_scorer`, which orders what its
//! pickers list.

use super::{Abort, Prompter};

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
            .with_page_size(page_size())
            .with_scorer(&path_scorer())
            .prompt()
            .map_err(abort)
    }
}

/// How many options to show: 15, fewer in a short terminal, as `inquire` does not clamp it. Six
/// rows are left for the prompt and help lines and the wrapping of long options.
fn page_size() -> usize {
    let rows = terminal_size::terminal_size_of(std::io::stderr()).map_or(24, |(_, h)| h.0);
    usize::from(rows).saturating_sub(6).clamp(3, 15)
}

/// Scores an option (a path) against what has been typed, for `inquire`: fuzzy, with the bonuses
/// of path matching. `inquire` sorts by score and leaves ties in no order, so a tie is broken
/// by depth (the shallower path first, once something is typed) and then by the option's
/// position, which is path order: `packages/solid` stays above `packages/solid-element`, and
/// `lib` above `third_party/x/lib`. With nothing typed the list stays in path order.
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
        let depth = match input.is_empty() {
            true => 0,
            false => path.matches('/').count() as i64,
        };
        Some(i64::from(score) * 1_000_000_000 - depth * 1_000_000 - position as i64)
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
