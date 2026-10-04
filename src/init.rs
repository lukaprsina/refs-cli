//! `refs init` (spec §10): the files a new Project starts with.

use std::path::Path;

use crate::config::{self, Config};
use crate::diagnostic::InitError;
use crate::project;

/// The `refs.toml` a new Project starts with: every key commented out, no repos.
const TEMPLATE: &str = r##"# Reference repos for coding agents. Add one with `refs add <url>`.

# [settings]
# agents_files = ["AGENTS.md"]
# references_dir = ".references"

# [groups.solid]
# name = "SolidJS"
# description = "What the group is for"

# [repos.solid]
# url = "https://github.com/solidjs/solid"
# group = "solid"
# ref = "main"
# paths = ["packages/solid/src"]
# packages = ["solid-js"]
# start = ["README.md"]
"##;

/// What `init` did, for the caller to report.
#[derive(Debug)]
pub struct Initialised {
    pub config_created: bool,
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
    project::ensure_exclude(root, config.settings.references_dir())?;
    Ok(Initialised { config_created })
}
