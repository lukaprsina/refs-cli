//! `refs init`: it needs no loaded project, as there may be none yet.

use std::path::Path;

use crate::diagnostic::Note;
use crate::project;
use crate::ui::{Console, fail};

const REMINDERS: &str = "Add a repo with `refs add`, e.g. `refs add gh:owner/repo`.
Linters, formatters and type checkers are yours to configure: exclude the references dir from them.
Claude Code reads AGENTS.md only when there is no CLAUDE.md; to use CLAUDE.md, list it in `agents_files`.";

pub(crate) fn run(start: &Path, here: bool, console: &mut Console) -> u8 {
    let result = project::init_root(start, here)
        .map_err(miette::Report::new)
        .and_then(|root| crate::init::init(&root).map_err(miette::Report::new));
    match result {
        Ok(done) => {
            if !done.in_git {
                console.report(miette::Report::new(Note::NoGitRepo));
            }
            if done.config_created {
                console.status(format_args!("created {}", project::CONFIG_FILE));
                console.status(REMINDERS);
            } else {
                console.status("nothing changed");
            }
            0
        }
        Err(report) => fail(report, console),
    }
}
