//! Why `add` or `update` ended before it edited anything (ADR 0008, 0012). `cli` is the one place
//! that turns it into words and an exit code.

use crate::ui::Abort;

#[derive(Debug)]
pub enum Failure {
    /// Something could not be read or looked at (the Registry, the remote or `refs.lock`), or the
    /// request cannot be done as given, or a question had to be asked with no one to ask: the
    /// report says what to do.
    Error(miette::Report),
    /// A question ended without an answer.
    Asked(Abort),
    /// The command line is wrong: the report says how.
    Usage(miette::Report),
}

impl<E: miette::Diagnostic + Send + Sync + 'static> From<E> for Failure {
    fn from(error: E) -> Failure {
        Failure::Error(miette::Report::new(error))
    }
}
