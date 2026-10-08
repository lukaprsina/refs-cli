//! Package registries (ADR 0009): `add npm:<name>`, `cargo:<name>` and `pypi:<name>` ask one
//! for the repository a package is published from. The prefix is input sugar; the Repo that is
//! stored has the git URL.

pub mod crates;
pub mod fake;
pub mod npm;
pub mod pypi;
mod url;

mod http;
pub use http::Http;

use crate::edit::AddRepo;

/// A package index that `add` can ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ecosystem {
    Npm,
    Cargo,
    Pypi,
}

impl Ecosystem {
    pub const ALL: [Ecosystem; 3] = [Ecosystem::Npm, Ecosystem::Cargo, Ecosystem::Pypi];

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

/// Why a registry's answer holds no repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The package has no repository URL, or none that can be used.
    NoRepository,
    /// The body is not the document the registry is known to send.
    Malformed(String),
}

/// The package asked of a registry, as a message names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub registry: &'static str,
    pub name: String,
}

impl Lookup {
    pub fn new(ecosystem: Ecosystem, name: &str) -> Self {
        Lookup {
            registry: ecosystem.registry(),
            name: name.to_owned(),
        }
    }
}

/// Why a registry could not say where a package lives.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, miette::Diagnostic)]
pub enum RegistryError {
    #[error("no registry is available here")]
    #[diagnostic(code(refs::registry::unavailable))]
    Unavailable,

    #[error("`{}` is not on {}", .lookup.name, .lookup.registry)]
    #[diagnostic(code(refs::registry::not_found))]
    NotFound { lookup: Lookup },

    #[error("{} has no repository for `{}`", .lookup.registry, .lookup.name)]
    #[diagnostic(
        code(refs::registry::no_repository),
        help("add it by its git URL instead")
    )]
    NoRepository { lookup: Lookup },

    #[error("could not ask {} about `{}`: {why}", .lookup.registry, .lookup.name)]
    #[diagnostic(code(refs::registry::request_failed))]
    Request { lookup: Lookup, why: String },

    #[error("{} answered {status} about `{}`", .lookup.registry, .lookup.name)]
    #[diagnostic(code(refs::registry::bad_status))]
    Status { lookup: Lookup, status: u16 },

    #[error(
        "{} sent an answer about `{}` that refs cannot read: {why}",
        .lookup.registry, .lookup.name
    )]
    #[diagnostic(code(refs::registry::malformed))]
    Malformed { lookup: Lookup, why: String },

    #[error("`{prefix}{name}` needs the network, which `--offline` forbids")]
    #[diagnostic(
        code(refs::registry::offline),
        help("add it by its git URL, or leave out `--offline`")
    )]
    Offline { prefix: &'static str, name: String },

    #[error(
        "{} gives `{url}` for `{}`, which cannot be used: {problem}",
        .lookup.registry, .lookup.name
    )]
    #[diagnostic(code(refs::registry::bad_url), help("add it by a git URL you trust"))]
    BadUrl {
        lookup: Lookup,
        url: String,
        problem: String,
    },
}

impl RegistryError {
    /// What a registry's `failure` means for `lookup`.
    fn failed(lookup: Lookup, failure: Failure) -> Self {
        match failure {
            Failure::NoRepository => RegistryError::NoRepository { lookup },
            Failure::Malformed(why) => RegistryError::Malformed { lookup, why },
        }
    }
}

/// Looks packages up. One call per `add`, for the registry's latest document.
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
/// URL, and the package's name and directory as defaults for what the command line left out.
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
            prefix: ecosystem.prefix(),
            name: name.to_owned(),
        });
    }
    let found = registry.lookup(ecosystem, name)?;
    if let Some((problem, _)) = crate::config::url_problem(&found.url) {
        return Err(RegistryError::BadUrl {
            lookup: Lookup::new(ecosystem, name),
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
