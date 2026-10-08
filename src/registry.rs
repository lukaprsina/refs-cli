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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ecosystem {
    Npm,
    Cargo,
    Pypi,
}

impl Ecosystem {
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

/// Why a registry could not say where a package lives.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, miette::Diagnostic)]
pub enum RegistryError {
    #[error("no registry is available here")]
    #[diagnostic(code(refs::registry::unavailable))]
    Unavailable,

    #[error("`{name}` is not on {registry}")]
    #[diagnostic(code(refs::registry::not_found))]
    NotFound {
        registry: &'static str,
        name: String,
    },

    #[error("{registry} has no repository for `{name}`")]
    #[diagnostic(
        code(refs::registry::no_repository),
        help("add it by its git URL instead")
    )]
    NoRepository {
        registry: &'static str,
        name: String,
    },

    #[error("could not ask {registry} about `{name}`: {why}")]
    #[diagnostic(code(refs::registry::request_failed))]
    Request {
        registry: &'static str,
        name: String,
        why: String,
    },

    #[error("{registry} answered {status} about `{name}`")]
    #[diagnostic(code(refs::registry::bad_status))]
    Status {
        registry: &'static str,
        name: String,
        status: u16,
    },

    #[error("{registry} sent an answer about `{name}` that refs cannot read: {why}")]
    #[diagnostic(code(refs::registry::malformed))]
    Malformed {
        registry: &'static str,
        name: String,
        why: String,
    },

    #[error("`{prefix}{name}` needs the network, which `--offline` forbids")]
    #[diagnostic(
        code(refs::registry::offline),
        help("add it by its git URL, or leave out `--offline`")
    )]
    Offline { prefix: &'static str, name: String },

    #[error("{registry} gives `{url}` for `{name}`, which cannot be used: {problem}")]
    #[diagnostic(code(refs::registry::bad_url), help("add it by a git URL you trust"))]
    BadUrl {
        registry: &'static str,
        name: String,
        url: String,
        problem: String,
    },
}

/// Looks packages up. One call per `add`; with a `version`, the registry's document for that
/// version, since `repository` and `directory` can differ between versions.
pub trait Registry {
    fn lookup(
        &self,
        ecosystem: Ecosystem,
        name: &str,
        version: Option<&str>,
    ) -> Result<Found, RegistryError>;
}

/// The registry of a run that has none: `cli::run_with`, which cannot reach the network.
pub struct Unavailable;

impl Registry for Unavailable {
    fn lookup(&self, _: Ecosystem, _: &str, _: Option<&str>) -> Result<Found, RegistryError> {
        Err(RegistryError::Unavailable)
    }
}

/// The ecosystem and package name a shorthand such as `npm:@scope/name` asks for.
fn parse_shorthand(url: &str) -> Option<(Ecosystem, &str)> {
    [
        ("npm:", Ecosystem::Npm),
        ("cargo:", Ecosystem::Cargo),
        ("pypi:", Ecosystem::Pypi),
    ]
    .into_iter()
    .find_map(|(prefix, ecosystem)| Some((ecosystem, url.strip_prefix(prefix)?)))
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
    let found = registry.lookup(ecosystem, name, None)?;
    if let Some((problem, _)) = crate::config::url_problem(&found.url) {
        return Err(RegistryError::BadUrl {
            registry: ecosystem.registry(),
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
