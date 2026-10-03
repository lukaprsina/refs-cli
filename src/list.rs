//! `refs list`: every repo in the config, grouped, with its ref, paths and whether it is
//! enabled. Reads the config only (no lock, no git).

use std::fmt::Write;

use crate::active::is_active;
use crate::config::{Config, RepoRef};

pub fn list(config: &Config) -> String {
    let width = config
        .repos
        .keys()
        .map(|id| id.as_ref().len())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (group_id, group) in &config.groups {
        let disabled = if group.enabled.unwrap_or(true) {
            ""
        } else {
            " disabled"
        };
        let _ = writeln!(
            out,
            "{} ({}){disabled}",
            group_id.as_ref(),
            group.name.as_ref()
        );
        let in_group = |r: &RepoRef| {
            r.repo
                .group
                .as_ref()
                .is_some_and(|g| g.as_ref() == group_id.as_ref())
        };
        repos(&mut out, config, width, in_group);
    }
    if config.repos.values().any(|r| r.group.is_none()) {
        out.push_str("ungrouped\n");
        repos(&mut out, config, width, |r| r.repo.group.is_none());
    }
    out
}

fn repos(out: &mut String, config: &Config, width: usize, wanted: impl Fn(&RepoRef) -> bool) {
    let repos = config.repos.iter().map(|(id, repo)| RepoRef {
        id: id.as_ref().as_str(),
        repo,
    });
    for r in repos.filter(|r| wanted(r)) {
        let paths = if r.repo.paths.is_empty() {
            "all".to_string()
        } else {
            r.repo.path_strings().join(", ")
        };
        let disabled = if is_active(config, r.repo) {
            ""
        } else {
            "  disabled"
        };
        let _ = writeln!(
            out,
            "  {:width$}  {}  {}  {paths}{disabled}",
            r.id,
            r.repo.url.as_ref(),
            r.repo.effective_ref(),
        );
    }
}
