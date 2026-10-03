//! `refs init` (spec §10): the files a new Project starts with. It composes the same pieces
//! `sync` uses, so the block it writes is the one `sync` would write for an empty config.

use std::path::Path;

use crate::active::active;
use crate::agent_file;
use crate::config::{self, Config};
use crate::diagnostic::InitError;
use crate::lock::Lock;
use crate::project;
use crate::render::render;

/// The `refs.toml` a new Project starts with: every key commented out, no repos.
const TEMPLATE: &str = r##"# Reference repos for coding agents. Add one with `refs add <url>`, then run `refs sync`.

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

/// Set up the Project at `root`: create `refs.toml` if absent, put the empty Managed block in
/// each Agent file, and exclude `references_dir` from git. Running it again changes nothing.
pub fn init(root: &Path) -> Result<Initialised, InitError> {
    let config_created = !root.join(project::CONFIG_FILE).exists();
    if config_created {
        project::write_config(root, TEMPLATE)?;
    }
    let config: Config = config::parse(&project::read_config(root)?)?;
    let dir = config.settings.references_dir();
    let block = render(&active(&config), &Lock::new(Vec::new()), dir)
        .expect("an empty lock covers no active repo of an empty config");
    for file in config.settings.agents_files() {
        let path = root.join(file);
        let text = agent_file::read(&path)?.unwrap_or_default();
        agent_file::write(&path, &agent_file::splice(&text, &block)?)?;
    }
    project::ensure_exclude(root, dir)?;
    Ok(Initialised { config_created })
}
