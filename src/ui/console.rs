//! Where a run's output goes, and how a failure ends it.

use std::fmt::Display;
use std::io::Write;

use super::Say;

pub(crate) const EXIT_ERROR: u8 = 1;
pub(crate) const EXIT_USAGE: u8 = 2;
pub(crate) const EXIT_OUT_OF_DATE: u8 = 3;

/// Where a run's output goes, in uv's style: status lines on stderr (silenced by `-q`),
/// problems on stderr always, and stdout for data only.
pub(crate) struct Console<'w> {
    out: &'w mut dyn Write,
    err: &'w mut dyn Write,
    quiet: bool,
}

impl<'w> Console<'w> {
    pub(crate) fn new(out: &'w mut dyn Write, err: &'w mut dyn Write, quiet: bool) -> Self {
        Console { out, err, quiet }
    }

    /// Whether `-q` was given.
    pub(super) fn quiet(&self) -> bool {
        self.quiet
    }

    /// Data: what the command is for, such as the `list` table.
    pub(crate) fn data(&mut self, text: &str) {
        // A closed pipe is the reader's choice, not an error of ours.
        let _ = write!(self.out, "{text}");
    }

    /// A status line: what changed, or that nothing did.
    pub(crate) fn status(&mut self, line: impl Display) {
        if !self.quiet {
            let _ = writeln!(self.err, "{line}");
        }
    }

    /// A problem, or a hint about one: always printed.
    pub(crate) fn problem(&mut self, line: impl Display) {
        let _ = writeln!(self.err, "{line}");
    }

    pub(crate) fn report(&mut self, report: impl std::fmt::Debug) {
        self.problem(format!("{report:?}"));
    }
}

impl Say for Console<'_> {
    fn status(&mut self, line: String) {
        Console::status(self, line);
    }

    fn warning(&mut self, line: String) {
        self.problem(format_args!("warning: {line}"));
    }

    fn shown(&mut self, line: String) {
        self.problem(line);
    }
}

/// Print `error` as a report and give the exit code for a failure.
pub(crate) fn fail(error: impl Into<miette::Report>, console: &mut Console) -> u8 {
    fail_all([error.into()], console)
}

/// Print every report and give the exit code for a failure.
pub(crate) fn fail_all(
    reports: impl IntoIterator<Item = miette::Report>,
    console: &mut Console,
) -> u8 {
    for report in reports {
        console.report(report);
    }
    EXIT_ERROR
}
