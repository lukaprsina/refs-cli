use crate::config::{Config, Group, Repo, RepoRef};

/// What `render` and `sync` act on: only active repos, grouped as the block shows them.
#[derive(Debug)]
pub struct ActiveSet<'a> {
    pub sections: Vec<Section<'a>>,
}

/// A group heading with its active repos in config order. `group` is `None` for the
/// trailing "Ungrouped" section.
#[derive(Debug)]
pub struct Section<'a> {
    pub group: Option<(&'a str, &'a Group)>,
    pub repos: Vec<RepoRef<'a>>,
}

pub fn active(config: &Config) -> ActiveSet<'_> {
    let repo_active = |repo: &Repo| repo.enabled.unwrap_or(true);
    let mut sections = Vec::new();
    for (group_id, group) in &config.groups {
        if !group.enabled.unwrap_or(true) {
            continue;
        }
        let repos: Vec<_> = config
            .repos
            .iter()
            .filter(|(_, r)| {
                repo_active(r)
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
        .filter(|(_, r)| repo_active(r) && r.group.is_none())
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
