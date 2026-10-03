use crate::config::{Config, Group, Repo, RepoRef};

/// What `render` and `sync` act on: only active repos, grouped as the block shows them.
#[derive(Debug)]
pub struct ActiveSet<'a> {
    pub sections: Vec<Section<'a>>,
}

impl<'a> ActiveSet<'a> {
    /// Every active repo, in block order.
    pub fn repos<'b>(&'b self) -> impl Iterator<Item = RepoRef<'a>> + 'b {
        self.sections.iter().flat_map(|s| s.repos.iter().copied())
    }

    pub fn get(&self, id: &str) -> Option<RepoRef<'a>> {
        self.repos().find(|r| r.id == id)
    }
}

/// A group heading with its active repos in config order. `group` is `None` for the
/// trailing "Ungrouped" section.
#[derive(Debug)]
pub struct Section<'a> {
    pub group: Option<(&'a str, &'a Group)>,
    pub repos: Vec<RepoRef<'a>>,
}

/// Whether `repo` is active (spec §6.4): not disabled itself, and not in a disabled group.
pub fn is_active(config: &Config, repo: &Repo) -> bool {
    let group_enabled = repo
        .group
        .as_ref()
        .and_then(|g| config.groups.get(g.as_ref().as_str()))
        .is_none_or(|group| group.enabled.unwrap_or(true));
    repo.enabled.unwrap_or(true) && group_enabled
}

pub fn active(config: &Config) -> ActiveSet<'_> {
    let mut sections = Vec::new();
    for (group_id, group) in &config.groups {
        let repos: Vec<_> = config
            .repos
            .iter()
            .filter(|(_, r)| {
                is_active(config, r)
                    && r.group
                        .as_ref()
                        .is_some_and(|g| g.as_ref() == group_id.as_ref())
            })
            .map(|(id, repo)| RepoRef {
                id: id.as_ref().as_str(),
                repo,
            })
            .collect();
        if !repos.is_empty() {
            sections.push(Section {
                group: Some((group_id.as_ref().as_str(), group)),
                repos,
            });
        }
    }
    let ungrouped: Vec<_> = config
        .repos
        .iter()
        .filter(|(_, r)| is_active(config, r) && r.group.is_none())
        .map(|(id, repo)| RepoRef {
            id: id.as_ref().as_str(),
            repo,
        })
        .collect();
    if !ungrouped.is_empty() {
        sections.push(Section {
            group: None,
            repos: ungrouped,
        });
    }
    ActiveSet { sections }
}
