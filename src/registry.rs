//! Package registries (ADR 0009): `add npm:<name>`, `cargo:<name>` and `pypi:<name>` ask one
//! for the repository a package is published from. The prefix is input sugar; the Repo that is
//! stored has the git URL. So is `gh:owner/repo` (`forge`), which asks nothing.

pub mod crates;
pub mod fake;
pub mod forge;
pub mod npm;
pub mod pin;
pub mod pypi;
pub mod tag;
mod url;
pub mod used;

mod http;
pub use http::{Http, interpret, request_url};

use crate::edit::AddRepo;

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
    #[error("no registry is available here")]
    #[diagnostic(code(refs::registry::unavailable))]
    Unavailable,

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

/// The registry of a run that has none: `cli::run_with`, which cannot reach the network.
pub struct Unavailable;

impl Registry for Unavailable {
    fn lookup(&self, _: &Package) -> Result<Found, RegistryError> {
        Err(RegistryError::Unavailable)
    }
}

/// Whether `url` is a shorthand `expand` would act on: a registry prefix and a name, or a forge
/// prefix. The prompt accepts it as a URL (`prompt::ask_url`).
pub fn is_shorthand(url: &str) -> bool {
    forge::name(url).is_some() || parse_shorthand(url).is_some_and(|(_, name, _)| !name.is_empty())
}

/// The ecosystem, package name and version a shorthand such as `npm:@scope/name@1.2.3` asks
/// for. The version is what follows the last `@` that does not start a scope, and may be empty
/// or not a version: `expand` refuses those.
fn parse_shorthand(url: &str) -> Option<(Ecosystem, &str, Option<&str>)> {
    let (ecosystem, rest) = Ecosystem::ALL
        .into_iter()
        .find_map(|ecosystem| Some((ecosystem, url.strip_prefix(ecosystem.spec().prefix)?)))?;
    Some(match rest.rfind('@').filter(|&at| at > 0) {
        Some(at) => (ecosystem, &rest[..at], Some(&rest[at + 1..])),
        None => (ecosystem, rest, None),
    })
}

/// `version` without a leading `v`, if it is an exact version: it starts with a digit and holds
/// only letters, digits and `.-+_`, and no `x` or `X` in its numbers. A dist-tag (`latest`) or a
/// range (`^1.2`, `>=1`, `*`, `1.x`) is not one, and has no tag to map to.
fn exact_version(version: &str) -> Result<&str, &'static str> {
    let bare = version.strip_prefix('v').unwrap_or(version);
    if bare.is_empty() {
        return Err("the version is empty");
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || ".-+_".contains(c);
    let numbers = bare.split(['-', '+']).next().unwrap_or(bare);
    let wildcard = numbers
        .split('.')
        .any(|part| part.eq_ignore_ascii_case("x"));
    if bare.starts_with(|c: char| c.is_ascii_digit()) && bare.chars().all(plain) && !wildcard {
        Ok(bare)
    } else {
        Err("an exact version such as `1.2.3` is needed, not a tag or a range")
    }
}

/// Whether `version` is a word such as `next` or `beta-2`, which a person means as a ref.
fn looks_like_tag(version: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || "-_.".contains(c);
    let v_number = version
        .strip_prefix('v')
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    version.starts_with(|c: char| c.is_ascii_alphabetic()) && !v_number && version.chars().all(word)
}

/// The package a shorthand named, and its version if the shorthand gave one, for finding the
/// tag it was released as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requested {
    pub package: Package,
    pub version: Option<String>,
}

/// An `add` after `expand`: the repo to add, and the package its shorthand named, if it was a
/// registry shorthand.
#[derive(Debug, Clone)]
pub struct Expanded {
    pub repo: AddRepo,
    pub requested: Option<Requested>,
    /// The registry's description of the package, to start the description question from.
    pub description: Option<String>,
    /// The path a forge shorthand had after the repository, which was dropped.
    pub ignored: Option<String>,
}

/// `repo` with a registry shorthand in its `url` replaced by what the registry says: the git
/// URL, and the package's directory and name as its `paths` and `packages`, unless the command
/// line gave them (ADR 0009).
/// Any other `repo` comes back as it is.
/// With `offline` a shorthand is refused, as it needs the network.
/// A version and a `ref` both pin, so a shorthand with a version is refused when `repo` has a `ref`.
/// The `ref` is not set here: a shorthand says which package it `requested`, and with a version
/// (given, or found in a Package lockfile, see `used::find`) `Source::tags` and
/// `tag::tag_for` turn that into a tag.
pub fn expand(
    repo: &AddRepo,
    registry: &dyn Registry,
    offline: bool,
) -> Result<Expanded, RegistryError> {
    if let Some(named) = forge::name(&repo.url) {
        let named = named.map_err(|why| RegistryError::BadForge {
            shorthand: repo.url.clone(),
            why: why.to_owned(),
        })?;
        let mut expanded = repo.clone();
        expanded.url = named.url;
        return Ok(Expanded {
            repo: expanded,
            requested: None,
            description: None,
            ignored: named.ignored,
        });
    }
    let Some((ecosystem, name, version)) = parse_shorthand(&repo.url) else {
        return Ok(Expanded {
            repo: repo.clone(),
            requested: None,
            description: None,
            ignored: None,
        });
    };
    let version = version
        .map(|version| {
            exact_version(version).map_err(|why| match looks_like_tag(version) {
                true => RegistryError::TagNotVersion {
                    tag: version.to_owned(),
                    bare: format!("{}{name}", ecosystem.spec().prefix),
                },
                false => RegistryError::BadVersion {
                    shorthand: repo.url.clone(),
                    why: why.to_owned(),
                },
            })
        })
        .transpose()?;
    if version.is_some() && repo.git_ref.is_some() {
        return Err(RegistryError::VersionAndRef {
            shorthand: repo.url.clone(),
        });
    }
    let package = Package::new(ecosystem, name);
    if offline {
        return Err(RegistryError::Offline { package });
    }
    let found = registry.lookup(&package)?;
    if let Some((problem, _)) = crate::config::url_problem(&found.url) {
        return Err(RegistryError::BadUrl {
            package,
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
        expanded.packages.push(package.name.clone());
    }
    Ok(Expanded {
        repo: expanded,
        requested: Some(Requested {
            package,
            version: version.map(str::to_owned),
        }),
        description: found.description,
        ignored: None,
    })
}
