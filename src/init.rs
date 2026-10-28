//! `refs init` (spec §10): the files a new Project starts with.

use std::path::Path;

use crate::config::{self, Config};
use crate::diagnostic::Note;
use crate::exclude;
use crate::project;
use crate::ui::{Console, fail};

/// The `refs.toml` a new Project starts with: every key commented out, no repos.
const TEMPLATE: &str = r##"# Reference repos for coding agents. Add one with `refs add`.

# tooling_ignore = ["prettier"]      # optional; tools to leave out of the tooling gap notes: prettier, eslint, black, biome, markdownlint, zed

# [settings]
# agents_files = ["AGENTS.md"]
# references_dir = ".references"

# [groups.solidjs-2]
# name = "SolidJS 2.0"
# description = "What the group is for"
# enabled = true

# [repos.solid]
# url = "https://github.com/solidjs/solid"
# group = "solidjs-2"
# ref = "next"
# description = "One line on what the repo is"
# paths = ["packages", "documentation"]
# packages = ["solid-js", "@solidjs/web", "@solidjs/signals"]
# start = ["documentation/solid-2.0/MIGRATION.md"]
# enabled = true
"##;

/// What `refs init` says after it created the config.
const REMINDERS: &str = "Add a repo with `refs add`, e.g. `refs add gh:owner/repo`.
Claude Code reads AGENTS.md only when there is no CLAUDE.md; to use CLAUDE.md, list it in `agents_files`.";

/// `refs init`, then what it did. It needs no loaded project, as there may be none yet.
///
/// It creates `refs.toml` if absent and excludes `references_dir` from git. The Managed block
/// is `sync`'s: with no Repo active there is none to write. Running it again changes nothing.
pub(crate) fn run(start: &Path, here: bool, console: &mut Console) -> u8 {
    match init(start, here, console) {
        Ok(()) => 0,
        Err(report) => fail(report, console),
    }
}

fn init(start: &Path, here: bool, console: &mut Console) -> Result<(), miette::Report> {
    let root = project::init_root(start, here)?;
    let config_created = !root.join(project::CONFIG_FILE).exists();
    if config_created {
        project::write_config(&root, TEMPLATE)?;
    }
    let config: Config = config::parse(&project::read_config(&root)?)?;
    let references_dir = config.settings.references_dir().to_owned();
    if !exclude::ensure(&root, &references_dir)? {
        console.report(miette::Report::new(Note::NoGitRepo));
    }
    if config_created {
        console.status(format_args!("created {}", project::CONFIG_FILE));
        console.status(REMINDERS);
        console.advice(miette::Report::new(Note::EditorHint { references_dir }));
    } else {
        console.status("nothing changed");
    }
    Ok(())
}
