//! Package registries (ADR 0009): `add npm:<name>`, `cargo:<name>` and `pypi:<name>` ask one
//! for the repository a package is published from. The prefix is input sugar; the Repo that is
//! stored has the git URL.

pub mod crates;
pub mod fake;
pub mod lockfile;
pub mod npm;
pub mod pypi;
pub mod tag;
mod url;

mod http;
pub use http::{Fetch, Http, Reply, Reqwest};

use crate::edit::AddRepo;

/// A package index that `add` can ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ecosystem {
    Npm,
    Cargo,
    Pypi,
}

impl Ecosystem {
    /// Every ecosystem, for finding the one a shorthand names. Add a new one here too.
    const ALL: [Ecosystem; 3] = [Ecosystem::Npm, Ecosystem::Cargo, Ecosystem::Pypi];

    /// The shorthand prefix.
    fn prefix(self) -> &'static str {
        match self {
            Ecosystem::Npm => "npm:",
            Ecosystem::Cargo => "cargo:",
            Ecosystem::Pypi => "pypi:",
        }
    }

    /// The registry's name in a message.
    pub fn registry(self) -> &'static str {
        match self {
            Ecosystem::Npm => "npm",
            Ecosystem::Cargo => "crates.io",
            Ecosystem::Pypi => "PyPI",
        }
    }
}

/// What a registry says about where a package lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// A normalised https git URL.
    pub url: String,
    /// The package's directory inside the repo, when the registry knows it (npm only).
    pub directory: Option<String>,
}

/// What an adapter makes of a registry's document: where the package lives, `None` when the
/// document names no repository that can be used, or why the body is not the document the
/// registry is known to send.
pub type Answer = Result<Option<Found>, String>;

/// The registry's document in `body`, or why it is not the one the registry is known to send.
fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, String> {
    serde_json::from_str(body).map_err(|e| e.to_string())
}

/// Why a registry could not say where a package lives.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, miette::Diagnostic)]
pub enum RegistryError {
    #[error("no registry is available here")]
    #[diagnostic(code(refs::registry::unavailable))]
    Unavailable,

    #[error("`{}` is not on {}", .name, .ecosystem.registry())]
    #[diagnostic(code(refs::registry::not_found))]
    NotFound { ecosystem: Ecosystem, name: String },

    #[error("{} has no repository for `{}`", .ecosystem.registry(), .name)]
    #[diagnostic(
        code(refs::registry::no_repository),
        help("add it by its git URL instead")
    )]
    NoRepository { ecosystem: Ecosystem, name: String },

    #[error("could not ask {} about `{}`: {why}", .ecosystem.registry(), .name)]
    #[diagnostic(code(refs::registry::request_failed))]
    Request {
        ecosystem: Ecosystem,
        name: String,
        why: String,
    },

    #[error("{} answered {status} about `{}`", .ecosystem.registry(), .name)]
    #[diagnostic(code(refs::registry::bad_status))]
    Status {
        ecosystem: Ecosystem,
        name: String,
        status: u16,
    },

    #[error(
        "{} sent an answer about `{}` that refs cannot read: {why}",
        .ecosystem.registry(), .name
    )]
    #[diagnostic(code(refs::registry::malformed))]
    Malformed {
        ecosystem: Ecosystem,
        name: String,
        why: String,
    },

    #[error(
        "`{}{name}` needs the network, which `--offline` forbids",
        .ecosystem.prefix()
    )]
    #[diagnostic(
        code(refs::registry::offline),
        help("add it by its git URL, or leave out `--offline`")
    )]
    Offline { ecosystem: Ecosystem, name: String },

    #[error("`{shorthand}` does not name a version: {why}")]
    #[diagnostic(
        code(refs::registry::bad_version),
        help(
            "give an exact version such as `1.2.3`, or leave it out to follow the default branch"
        )
    )]
    BadVersion { shorthand: String, why: String },

    #[error(
        "{} gives `{url}` for `{}`, which cannot be used: {problem}",
        .ecosystem.registry(), .name
    )]
    #[diagnostic(code(refs::registry::bad_url), help("add it by a git URL you trust"))]
    BadUrl {
        ecosystem: Ecosystem,
        name: String,
        url: String,
        problem: String,
    },
}

/// Looks packages up. One call per `add`, for the document that names the package's repository.
pub trait Registry {
    fn lookup(&self, ecosystem: Ecosystem, name: &str) -> Result<Found, RegistryError>;
}

/// The registry of a run that has none: `cli::run_with`, which cannot reach the network.
pub struct Unavailable;

impl Registry for Unavailable {
    fn lookup(&self, _: Ecosystem, _: &str) -> Result<Found, RegistryError> {
        Err(RegistryError::Unavailable)
    }
}

/// The ecosystem, package name and version a shorthand such as `npm:@scope/name@1.2.3` asks
/// for. The version is what follows the last `@` that does not start a scope, and may be empty
/// or not a version: `expand` refuses those.
fn parse_shorthand(url: &str) -> Option<(Ecosystem, &str, Option<&str>)> {
    let (ecosystem, rest) = Ecosystem::ALL
        .into_iter()
        .find_map(|ecosystem| Some((ecosystem, url.strip_prefix(ecosystem.prefix())?)))?;
    Some(match rest.rfind('@').filter(|&at| at > 0) {
        Some(at) => (ecosystem, &rest[..at], Some(&rest[at + 1..])),
        None => (ecosystem, rest, None),
    })
}

/// `version` without a leading `v`, if it is an exact version: it starts with a digit and holds
/// only letters, digits and `.-+_`. A dist-tag (`latest`) or a range (`^1.2`, `>=1`, `*`) is not
/// one, and has no tag to map to.
fn exact_version(version: &str) -> Result<&str, &'static str> {
    let bare = version.strip_prefix('v').unwrap_or(version);
    if bare.is_empty() {
        return Err("the version is empty");
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || ".-+_".contains(c);
    if bare.starts_with(|c: char| c.is_ascii_digit()) && bare.chars().all(plain) {
        Ok(bare)
    } else {
        Err("an exact version such as `1.2.3` is needed, not a tag or a range")
    }
}

/// The package a shorthand named, and its version if the shorthand gave one, for finding the
/// tag it was released as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub ecosystem: Ecosystem,
    pub name: String,
    pub version: Option<String>,
}

/// An `add` after `expand`: the repo to add, and the release its shorthand named, if it was a
/// shorthand.
#[derive(Debug, Clone)]
pub struct Expanded {
    pub repo: AddRepo,
    pub release: Option<Release>,
}

/// `repo` with a registry shorthand in its `url` replaced by what the registry says: the git
/// URL, and the package's directory and name as its `paths` and `packages`, unless the command
/// line gave them (ADR 0009).
/// Any other `repo` comes back as it is.
/// With `offline` a shorthand is refused, as it needs the network.
/// The `ref` is not set here: a shorthand says which `release` it names, and with a version
/// (given, or found in a Package lockfile, see `lockfile::find`) `Source::tags` and
/// `tag::tag_for` turn that into a tag.
pub fn expand(
    repo: &AddRepo,
    registry: &dyn Registry,
    offline: bool,
) -> Result<Expanded, RegistryError> {
    let Some((ecosystem, name, version)) = parse_shorthand(&repo.url) else {
        return Ok(Expanded {
            repo: repo.clone(),
            release: None,
        });
    };
    let version = version
        .map(|version| {
            exact_version(version).map_err(|why| RegistryError::BadVersion {
                shorthand: repo.url.clone(),
                why: why.to_owned(),
            })
        })
        .transpose()?;
    if offline {
        return Err(RegistryError::Offline {
            ecosystem,
            name: name.to_owned(),
        });
    }
    let found = registry.lookup(ecosystem, name)?;
    if let Some((problem, _)) = crate::config::url_problem(&found.url) {
        return Err(RegistryError::BadUrl {
            ecosystem,
            name: name.to_owned(),
            url: found.url,
            problem: problem.to_owned(),
        });
    }
    let mut expanded = repo.clone();
    expanded.url = found.url;
    if expanded.paths.is_empty() {
        expanded.paths.extend(found.directory);
    }
    if expanded.packages.is_empty() {
        expanded.packages.push(name.to_owned());
    }
    Ok(Expanded {
        repo: expanded,
        release: Some(Release {
            ecosystem,
            name: name.to_owned(),
            version: version.map(str::to_owned),
        }),
    })
}
