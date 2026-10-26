//! What `add` makes of a shorthand in its `url`: `npm:name@1.2.3`, `cargo:`, `pypi:` or a forge
//! prefix (`gh:owner/repo`). A registry shorthand is looked up and replaced by the git URL it
//! names (ADR 0009), and the package and version it asked for are kept to pin to a tag
//! (`add::pin`). The prefix is input sugar; the Repo that is stored has the git URL.

use crate::edit::AddRepo;
use crate::registry::{Package, Registry, RegistryError, forge, parse_shorthand};
use crate::ui::Say;

/// Whether `url` is a shorthand `expand` would act on: a registry prefix and a name, or a forge
/// prefix. The prompt accepts it as a URL (`add::ask::ask_url`).
pub(super) fn is_shorthand(url: &str) -> bool {
    forge::name(url).is_some() || parse_shorthand(url).is_some_and(|(_, name, _)| !name.is_empty())
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
pub(super) struct Requested {
    pub(super) package: Package,
    pub(super) version: Option<String>,
}

/// An `add` after `expand`: the repo to add, and the package its shorthand named, if it was a
/// registry shorthand.
#[derive(Debug, Clone)]
pub(super) struct Expanded {
    pub(super) repo: AddRepo,
    pub(super) requested: Option<Requested>,
    /// The registry's description of the package, to start the description question from.
    pub(super) description: Option<String>,
}

/// `repo` with a registry shorthand in its `url` replaced by what the registry says: the git
/// URL, and the package's directory and name as its `paths` and `packages`, unless the command
/// line gave them (ADR 0009).
/// Any other `repo` comes back as it is.
/// With `offline` a shorthand is refused, as it needs the network.
/// A forge shorthand that had a path after the repository gets a warning through `say`: the path
/// is dropped.
/// A version and a `ref` both pin, so a shorthand with a version is refused when `repo` has a `ref`.
/// The `ref` is not set here: a shorthand says which package it `requested`, and with a version
/// (given, or found in a Package lockfile, see `used::answer`) `Source::tags` and
/// `tag::tag_for` turn that into a tag.
pub(super) fn expand(
    repo: &AddRepo,
    registry: &dyn Registry,
    offline: bool,
    say: &mut dyn Say,
) -> Result<Expanded, RegistryError> {
    if let Some(named) = forge::name(&repo.url) {
        let named = named.map_err(|why| RegistryError::BadForge {
            shorthand: repo.url.clone(),
            why: why.to_owned(),
        })?;
        if let Some(ignored) = &named.ignored {
            let shorthand = &repo.url;
            say.warning(format!(
                "ignored `{ignored}` after the repository in `{shorthand}`; \
                 use `--ref` and `--paths` to choose a ref and directories"
            ));
        }
        let mut expanded = repo.clone();
        expanded.url = named.url;
        return Ok(Expanded {
            repo: expanded,
            requested: None,
            description: None,
        });
    }
    let Some((ecosystem, name, version)) = parse_shorthand(&repo.url) else {
        return Ok(Expanded {
            repo: repo.clone(),
            requested: None,
            description: None,
        });
    };
    let version = version
        .map(|version| {
            exact_version(version).map_err(|why| match looks_like_tag(version) {
                true => RegistryError::TagNotVersion {
                    tag: version.to_owned(),
                    bare: Package::new(ecosystem, name).to_string(),
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
    expanded.origin.get_or_insert_with(|| package.to_string());
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
    })
}
