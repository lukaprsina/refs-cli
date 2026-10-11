//! What `add` pins a registry package to (spec §7.2): the version given or the one a Package
//! lockfile says the project uses, and the Version tag it was released as. It decides; `cli`
//! words the result and asks the two questions it leaves open.

use std::path::{Path, PathBuf};

use super::Package;
use super::tag::tag_for;
use super::used::{self, Lookup, Pick};
use crate::diagnostic::SourceError;
use crate::source::Source;

/// Where the remote's tags come from. Every `Source` lists them; `NoTags` stands in where
/// nothing can be listed (`--no-sync`).
pub trait Tags {
    /// The remote's tag names, or `None` when this cannot list them.
    fn remote_tags(&self, url: &str) -> Result<Option<Vec<String>>, SourceError>;
}

impl<S: Source + ?Sized> Tags for S {
    fn remote_tags(&self, url: &str) -> Result<Option<Vec<String>>, SourceError> {
        self.tags(url).map(Some)
    }
}

/// Lists no tags.
pub struct NoTags;

impl Tags for NoTags {
    fn remote_tags(&self, _: &str) -> Result<Option<Vec<String>>, SourceError> {
        Ok(None)
    }
}

/// The Package lockfile that was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub file: PathBuf,
    /// Package lockfiles of the same ecosystem in the same directory that were not read.
    pub ignored: Vec<PathBuf>,
    /// The version the project uses; `None` when the file has no such package, or several.
    pub version: Option<String>,
}

/// Why the repo follows the remote's default branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// No Package lockfile of the ecosystem was found.
    NoLockfile,
    /// The nearest Package lockfile could not be read.
    Unreadable { file: PathBuf, why: String },
    /// The Package lockfile `file` does not have the package.
    NotInLockfile { file: PathBuf },
    /// `version` has no tag in the remote.
    Untagged { version: String },
    /// `version` could not be looked up, as no tags can be listed.
    Unlisted { version: String },
}

/// What a version is pinned to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Pin to `tag`, which `version` was released as.
    Tag { version: String, tag: String },
    /// Follow the remote's default branch.
    DefaultBranch(Why),
}

/// What `pin` came to: one version settled, or several left to choose from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Settled(Choice),
    /// The Package lockfile `file` has several versions, lowest first, and nothing to choose
    /// between them: call `pin_version` with the one chosen.
    Several {
        file: PathBuf,
        versions: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    /// The Package lockfile that was read, if one was.
    pub read: Option<Read>,
    pub decision: Decision,
}

/// The tags of the remote could not be listed. `read` is what was read from the Package
/// lockfile before that, for saying so before the error.
#[derive(Debug, Clone)]
pub struct Failed {
    pub read: Option<Read>,
    pub error: SourceError,
}

impl Failed {
    fn new(read: Option<Read>, error: SourceError) -> Box<Self> {
        Box::new(Failed { read, error })
    }
}

/// What to pin `package`, in the repo at `url`, to. With a `version` that is the one used;
/// without, the project at `root` is asked through its Package lockfile (`used::find`, then
/// `Found::pick`). The tag for the version comes from `tags` (`tag_for`).
pub fn pin<T: Tags + ?Sized>(
    package: &Package,
    version: Option<&str>,
    url: &str,
    root: &Path,
    tags: &T,
) -> Result<Pinned, Box<Failed>> {
    let settled = |read, choice| Pinned {
        read,
        decision: Decision::Settled(choice),
    };
    if let Some(version) = version {
        let choice = pin_version(package, version, url, tags)
            .map_err(|error| Failed { read: None, error })?;
        return Ok(settled(None, choice));
    }
    let found = match used::find(package, root) {
        Lookup::Found(found) => found,
        Lookup::Missing => return Ok(settled(None, Choice::DefaultBranch(Why::NoLockfile))),
        Lookup::Unreadable { file, why } => {
            let why = Why::Unreadable { file, why };
            return Ok(settled(None, Choice::DefaultBranch(why)));
        }
    };
    let mut read = Read {
        file: found.file.clone(),
        ignored: found.ignored.clone(),
        version: None,
    };
    match found.pick() {
        Pick::Nothing => {
            let why = Why::NotInLockfile { file: found.file };
            Ok(settled(Some(read), Choice::DefaultBranch(why)))
        }
        Pick::Several(versions) => {
            let versions = versions.into_iter().map(str::to_owned).collect();
            Ok(Pinned {
                read: Some(read),
                decision: Decision::Several {
                    file: found.file,
                    versions,
                },
            })
        }
        Pick::One(version) => {
            read.version = Some(version.to_owned());
            match pin_version(package, version, url, tags) {
                Ok(choice) => Ok(settled(Some(read), choice)),
                Err(error) => Err(Failed::new(Some(read), error)),
            }
        }
    }
}

/// What to pin `version` of `package`, in the repo at `url`, to: the tag it was released as, if
/// `tags` lists one.
pub fn pin_version<T: Tags + ?Sized>(
    package: &Package,
    version: &str,
    url: &str,
    tags: &T,
) -> Result<Choice, SourceError> {
    let version = version.to_owned();
    let Some(tags) = tags.remote_tags(url)? else {
        return Ok(Choice::DefaultBranch(Why::Unlisted { version }));
    };
    Ok(match tag_for(&package.name, &version, &tags) {
        Some(tag) => Choice::Tag {
            tag: tag.to_owned(),
            version,
        },
        None => Choice::DefaultBranch(Why::Untagged { version }),
    })
}
