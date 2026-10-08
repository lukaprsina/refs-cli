//! Package registries (ADR 0009): `add npm:<name>`, `cargo:<name>` and `pypi:<name>` ask one
//! for the repository a package is published from. The prefix is input sugar; the Repo that is
//! stored has the git URL.

pub mod crates;
pub mod fake;
pub mod npm;
pub mod pypi;
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

/// The ecosystem and package name a shorthand such as `npm:@scope/name` asks for.
fn parse_shorthand(url: &str) -> Option<(Ecosystem, &str)> {
    Ecosystem::ALL
        .into_iter()
        .find_map(|ecosystem| Some((ecosystem, url.strip_prefix(ecosystem.prefix())?)))
}

/// `repo` with a registry shorthand in its `url` replaced by what the registry says: the git
/// URL, and the package's directory and name as its `paths` and `packages`, unless the command
/// line gave them (ADR 0009).
/// Any other `repo` comes back as it is.
/// With `offline` a shorthand is refused, as it needs the network.
pub fn expand(
    repo: &AddRepo,
    registry: &dyn Registry,
    offline: bool,
) -> Result<AddRepo, RegistryError> {
    let Some((ecosystem, name)) = parse_shorthand(&repo.url) else {
        return Ok(repo.clone());
    };
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
    Ok(expanded)
}
