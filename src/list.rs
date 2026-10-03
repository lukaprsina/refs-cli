//! `refs list`: every repo in the config, grouped, with its ref, paths and whether it is
//! enabled. Pure: `list` reads the config only; `list_status` also takes the Lock and what is
//! on disk, which `sync::status` collects.

use std::collections::HashMap;
use std::fmt::Write;

use crate::active::is_active;
use crate::config::{Config, RepoRef};
use crate::lock::Lock;
use crate::source::Observed;

/// What `refs list --status` adds to the config: the Lock and what is on disk, by repo id
/// (an id with no entry counts as absent).
pub struct Status {
    pub lock: Option<Lock>,
    pub observed: HashMap<String, Observed>,
}

pub fn list(config: &Config) -> String {
    render(config, None)
}

/// `list` with the locked SHA (short) and the state of each checkout: ok, missing, wrong
/// SHA, foreign, not locked, or disabled.
pub fn list_status(config: &Config, status: &Status) -> String {
    render(config, Some(status))
}

fn render(config: &Config, status: Option<&Status>) -> String {
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
        repos(&mut out, config, width, status, in_group);
    }
    if config.repos.values().any(|r| r.group.is_none()) {
        out.push_str("ungrouped\n");
        repos(&mut out, config, width, status, |r| r.repo.group.is_none());
    }
    out
}

fn repos(
    out: &mut String,
    config: &Config,
    width: usize,
    status: Option<&Status>,
    wanted: impl Fn(&RepoRef) -> bool,
) {
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
        let tail = match status {
            Some(status) => status_columns(config, status, &r),
            None if is_active(config, r.repo) => String::new(),
            None => "  disabled".to_string(),
        };
        let _ = writeln!(
            out,
            "  {:width$}  {}  {}  {paths}{tail}",
            r.id,
            r.repo.url.as_ref(),
            r.repo.effective_ref(),
        );
    }
}

/// Where one repo stands against the Lock and the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutState {
    Ok,
    /// Locked, but nothing (or a dangling directory) is checked out.
    Missing,
    WrongSha,
    /// A directory that is not one of ours is in the way.
    Foreign,
    NotLocked,
    Disabled,
}

impl std::fmt::Display for CheckoutState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            CheckoutState::Ok => "ok",
            CheckoutState::Missing => "missing",
            CheckoutState::WrongSha => "wrong SHA",
            CheckoutState::Foreign => "foreign",
            CheckoutState::NotLocked => "not locked",
            CheckoutState::Disabled => "disabled",
        })
    }
}

fn checkout_state(config: &Config, status: &Status, r: &RepoRef) -> CheckoutState {
    if !is_active(config, r.repo) {
        return CheckoutState::Disabled;
    }
    let Some(locked) = status.lock.as_ref().and_then(|lock| lock.get(r.id)) else {
        return CheckoutState::NotLocked;
    };
    match status.observed.get(r.id) {
        None | Some(Observed::Absent | Observed::Dangling) => CheckoutState::Missing,
        Some(Observed::Foreign) => CheckoutState::Foreign,
        Some(Observed::At { pin, .. }) if pin.same_commit(&locked.pin) => CheckoutState::Ok,
        Some(Observed::At { .. }) => CheckoutState::WrongSha,
    }
}

/// `  <short locked SHA or ->  <state>` for one repo.
fn status_columns(config: &Config, status: &Status, r: &RepoRef) -> String {
    let sha = status
        .lock
        .as_ref()
        .and_then(|lock| lock.get(r.id))
        .map_or("-", |l| l.pin.short_id());
    format!("  {sha}  {}", checkout_state(config, status, r))
}
