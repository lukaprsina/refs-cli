//! `refs init` (spec §10): the files a new Project starts with.

use std::path::Path;

use crate::config::{self, Config};
use crate::diagnostic::InitError;
use crate::exclude;
use crate::project;

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

/// What `init` did, for the caller to report.
#[derive(Debug)]
pub struct Initialised {
    pub config_created: bool,
    /// Whether the Project is in a git worktree, so the exclude rule could be written.
    pub in_git: bool,
    /// The Project's references directory, for what is said about it.
    pub references_dir: String,
}

/// Set up the Project at `root`: create `refs.toml` if absent and exclude `references_dir`
/// from git. The Managed block is `sync`'s: with no Repo active there is none to write.
/// Running it again changes nothing.
pub fn init(root: &Path) -> Result<Initialised, InitError> {
    let config_created = !root.join(project::CONFIG_FILE).exists();
    if config_created {
        project::write_config(root, TEMPLATE)?;
    }
    let config: Config = config::parse(&project::read_config(root)?)?;
    let references_dir = config.settings.references_dir().to_owned();
    let in_git = exclude::ensure(root, &references_dir)?;
    Ok(Initialised {
        config_created,
        in_git,
        references_dir,
    })
}
