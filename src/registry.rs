//! Package registries (ADR 0009): `add npm:<name>`, `cargo:<name>` and `pypi:<name>` ask one
//! for the repository a package is published from. The prefix is input sugar; the Repo that is
//! stored has the git URL. So is `gh:owner/repo` (`forge`), which asks nothing.

pub mod crates;
#[cfg(any(test, feature = "testing"))]
pub mod fake;
pub mod forge;
pub mod npm;
pub mod pypi;
pub mod tag;
mod url;
pub mod used;

mod http;
pub use http::{Http, interpret, request_url};

/// A package index that `add` can ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ecosystem {
    Npm,
    Cargo,
    Pypi,
}

/// What an ecosystem is to refs. Each adapter module (`npm`, `crates`, `pypi`) holds its own.
struct Spec {
    /// The shorthand prefix.
    prefix: &'static str,
    /// The registry's name in a message.
    registry: &'static str,
    /// The URL of the document that holds the repository, the cheapest one the registry
    /// serves, with `{name}` for the package's name.
    url: &'static str,
    /// Reads that document.
    found: fn(&str) -> Answer,
    /// The ecosystem's Package lockfiles, as file name and format, in the order they are
    /// preferred when a directory holds more than one.
    lockfiles: &'static [(&'static str, used::Parser)],
}

impl Ecosystem {
    /// Every ecosystem, for finding the one a shorthand names.
    const ALL: [Ecosystem; 3] = [Ecosystem::Npm, Ecosystem::Cargo, Ecosystem::Pypi];

    /// Add a new ecosystem here and in `ALL`, with an adapter module that holds its `SPEC`.
    fn spec(self) -> &'static Spec {
        match self {
            Ecosystem::Npm => &npm::SPEC,
            Ecosystem::Cargo => &crates::SPEC,
            Ecosystem::Pypi => &pypi::SPEC,
        }
    }

    /// The registry's name in a message.
    pub fn registry(self) -> &'static str {
        self.spec().registry
    }
}

/// A package as a registry knows it: the ecosystem and the name together.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Package {
    pub ecosystem: Ecosystem,
    pub name: String,
}

impl Package {
    pub fn new(ecosystem: Ecosystem, name: impl Into<String>) -> Self {
        Package {
            ecosystem,
            name: name.into(),
        }
    }

    /// The Package an `origin` in `refs.toml` names (GLOSSARY: **Origin**): a registry prefix and
    /// a name, as `Display` writes it. No version, no URL, and no name a registry could not hold.
    pub fn from_origin(origin: &str) -> Result<Package, &'static str> {
        let (ecosystem, name, version) =
            parse_shorthand(origin).ok_or("an origin starts with `npm:`, `cargo:` or `pypi:`")?;
        if version.is_some() {
            return Err("an origin names a package, not a version");
        }
        if name.is_empty()
            || name.contains(|c: char| c.is_whitespace() || c.is_control() || c == ':')
        {
            return Err("an origin is a prefix and a package name, such as `npm:solid-js`");
        }
        Ok(Package::new(ecosystem, name))
    }
}

impl std::fmt::Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.ecosystem.spec().prefix, self.name)
    }
}

/// What a registry says about where a package lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// A normalised https git URL.
    pub url: String,
    /// The package's directory inside the repo, when the registry knows it (npm only).
    pub directory: Option<String>,
    /// What the registry says the package is, when it says (a blank is nothing).
    pub description: Option<String>,
}

/// What an adapter makes of a registry's document: where the package lives, `None` when the
/// document names no repository that can be used, or why the body is not the document the
/// registry is known to send.
pub type Answer = Result<Option<Found>, String>;

/// `text` unless it is missing or only whitespace.
fn non_blank(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// The registry's document in `body`, or why it is not the one the registry is known to send.
fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, String> {
    serde_json::from_str(body).map_err(|e| e.to_string())
}

/// Why a registry could not say where a package lives.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, miette::Diagnostic)]
pub enum RegistryError {
    #[error("`{}` is not on {}", .package.name, .package.ecosystem.registry())]
    #[diagnostic(code(refs::registry::not_found))]
    NotFound { package: Package },

    #[error("{} has no repository for `{}`", .package.ecosystem.registry(), .package.name)]
    #[diagnostic(
        code(refs::registry::no_repository),
        help("add it by its git URL instead")
    )]
    NoRepository { package: Package },

    #[error("could not ask {} about `{}`: {why}", .package.ecosystem.registry(), .package.name)]
    #[diagnostic(code(refs::registry::request_failed))]
    Request { package: Package, why: String },

    #[error("{} answered {status} about `{}`", .package.ecosystem.registry(), .package.name)]
    #[diagnostic(code(refs::registry::bad_status))]
    Status { package: Package, status: u16 },

    #[error(
        "{} sent an answer about `{}` that refs cannot read: {why}",
        .package.ecosystem.registry(), .package.name
    )]
    #[diagnostic(code(refs::registry::malformed))]
    Malformed { package: Package, why: String },

    #[error(
        "`{}{}` needs the network, which `--offline` forbids",
        .package.ecosystem.spec().prefix, .package.name
    )]
    #[diagnostic(
        code(refs::registry::offline),
        help("add it by its git URL, or leave out `--offline`")
    )]
    Offline { package: Package },

    #[error("`{shorthand}` does not name a repository: {why}")]
    #[diagnostic(
        code(refs::registry::bad_forge),
        help("write it as `gh:owner/repo` or `gl:group/sub/repo`, or give the full git URL")
    )]
    BadForge { shorthand: String, why: String },

    #[error("`{shorthand}` names a version and `--ref` names a ref; they cannot both pin")]
    #[diagnostic(
        code(refs::registry::version_and_ref),
        help("drop `--ref` to pin the version's tag, or drop the version to use the ref")
    )]
    VersionAndRef { shorthand: String },

    #[error("`{shorthand}` does not name a version: {why}")]
    #[diagnostic(
        code(refs::registry::bad_version),
        help(
            "give an exact version such as `1.2.3`, or leave it out to follow the default branch"
        )
    )]
    BadVersion { shorthand: String, why: String },

    #[error("`{tag}` looks like a tag, not a version")]
    #[diagnostic(
        code(refs::registry::tag_not_version),
        help(
            "to follow a git ref named `{tag}`, which may not be the dist-tag, write `{bare} --ref {tag}`"
        )
    )]
    TagNotVersion { tag: String, bare: String },

    #[error(
        "{} gives `{url}` for `{}`, which cannot be used: {problem}",
        .package.ecosystem.registry(), .package.name
    )]
    #[diagnostic(code(refs::registry::bad_url), help("add it by a git URL you trust"))]
    BadUrl {
        package: Package,
        url: String,
        problem: String,
    },
}

/// Looks packages up. One call per `add`, for the document that names the package's repository.
pub trait Registry {
    fn lookup(&self, package: &Package) -> Result<Found, RegistryError>;
}

/// The ecosystem, package name and version a shorthand such as `npm:@scope/name@1.2.3` asks
/// for. The version is what follows the last `@` that does not start a scope, and may be empty
/// or not a version: `add::expand` refuses those.
pub(crate) fn parse_shorthand(url: &str) -> Option<(Ecosystem, &str, Option<&str>)> {
    let (ecosystem, rest) = Ecosystem::ALL
        .into_iter()
        .find_map(|ecosystem| Some((ecosystem, url.strip_prefix(ecosystem.spec().prefix)?)))?;
    Some(match rest.rfind('@').filter(|&at| at > 0) {
        Some(at) => (ecosystem, &rest[..at], Some(&rest[at + 1..])),
        None => (ecosystem, rest, None),
    })
}
