use crate::config::{Config, Group, Id, Repo, RepoRef};

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

    /// The set without the Repos named in `ids`; a Section left empty goes.
    pub fn without(&self, ids: &[String]) -> ActiveSet<'a> {
        let sections = self
            .sections
            .iter()
            .filter_map(|s| {
                let repos: Vec<_> = s
                    .repos
                    .iter()
                    .copied()
                    .filter(|r| !ids.iter().any(|id| id == r.id))
                    .collect();
                (!repos.is_empty()).then(|| Section {
                    group: s.group,
                    repos,
                })
            })
            .collect();
        ActiveSet { sections }
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

/// A group heading in the full layout, with whether the group itself is enabled.
#[derive(Debug)]
pub struct LaidGroup<'a> {
    pub id: &'a str,
    pub group: &'a Group,
    pub enabled: bool,
}

/// A repo in the full layout. `enabled` is effective: false also when its group is disabled.
#[derive(Debug)]
pub struct LaidRepo<'a> {
    pub repo: RepoRef<'a>,
    pub enabled: bool,
}

/// One group (`None`: the trailing ungrouped section) with all its repos, enabled or not.
#[derive(Debug)]
pub struct LaidSection<'a> {
    pub group: Option<LaidGroup<'a>>,
    pub repos: Vec<LaidRepo<'a>>,
}

/// Every group in config order, then the ungrouped repos (only if there are any), each repo
/// tagged enabled or disabled. The one place the rule "enabled unless set false, in an
/// enabled group" is written (spec §6.4); the active set and `list` both come from it.
pub fn layout(config: &Config) -> Vec<LaidSection<'_>> {
    fn laid<'a>(id: &'a Id, repo: &'a Repo, group_enabled: bool) -> LaidRepo<'a> {
        LaidRepo {
            repo: RepoRef {
                id: id.as_ref().as_str(),
                repo,
            },
            enabled: group_enabled && repo.enabled.unwrap_or(true),
        }
    }
    let mut sections: Vec<_> = config
        .groups
        .iter()
        .map(|(group_id, group)| {
            let enabled = group.enabled.unwrap_or(true);
            LaidSection {
                group: Some(LaidGroup {
                    id: group_id.as_ref().as_str(),
                    group,
                    enabled,
                }),
                repos: config
                    .repos
                    .iter()
                    .filter(|(_, r)| {
                        r.group
                            .as_ref()
                            .is_some_and(|g| g.as_ref() == group_id.as_ref())
                    })
                    .map(|(id, r)| laid(id, r, enabled))
                    .collect(),
            }
        })
        .collect();
    let ungrouped: Vec<_> = config
        .repos
        .iter()
        .filter(|(_, r)| r.group.is_none())
        .map(|(id, r)| laid(id, r, true))
        .collect();
    if !ungrouped.is_empty() {
        sections.push(LaidSection {
            group: None,
            repos: ungrouped,
        });
    }
    sections
}

/// The enabled part of the layout; groups left with no repos have no section.
pub fn active(config: &Config) -> ActiveSet<'_> {
    let sections = layout(config)
        .into_iter()
        .filter_map(|section| {
            let repos: Vec<_> = section
                .repos
                .into_iter()
                .filter(|r| r.enabled)
                .map(|r| r.repo)
                .collect();
            (!repos.is_empty()).then(|| Section {
                group: section.group.map(|g| (g.id, g.group)),
                repos,
            })
        })
        .collect();
    ActiveSet { sections }
}
