//! What talks to the person. The `Prompter` seam (ADR 0008): the questions go through it, so
//! tests script the answers and only `cli::run` puts a terminal behind it; `Terminal` is the
//! `inquire` adapter, and `path_scorer` orders what its pickers list. `Say` is where a run says
//! what it read and decided. The questions themselves live with `add` and `update`; the pickers
//! they share are in `pickers`.

mod console;
pub(crate) mod pickers;
mod report;
mod terminal;

pub(crate) use console::{Console, EXIT_ERROR, EXIT_OUT_OF_DATE, fail, fail_all};
pub(crate) use report::{failed, print_report};
pub use terminal::{Terminal, path_scorer};

/// Where a run says what it read and decided (`pin`, the manifests, the pickers).
pub trait Say {
    /// What was read or decided.
    fn status(&mut self, line: String);
    /// Something the person should see even when the run is quiet.
    fn warning(&mut self, line: String);
    /// What a question is about: shown even when the run is quiet.
    fn shown(&mut self, line: String);
}

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
