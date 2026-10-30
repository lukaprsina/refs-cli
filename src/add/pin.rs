//! What `add` pins a registry package to (spec §7.2): the version given or the one a Package
//! lockfile says the project uses, and the Version tag it was released as. It decides, says what
//! it read and decided as it goes (`Say`, so a line comes before the question it leads to), and
//! asks the two questions that can come up through the `Prompter`: which of several versions,
//! and whether a version with no tag may follow the default branch.

use std::path::Path;

use crate::diagnostic::AddError;
use crate::failure::Failure;
use crate::registry::Package;
use crate::registry::tag::tag_for;
use crate::registry::used::{self, Answer};
use crate::source::Source;
use crate::ui::pickers::{ask_version, several_versions_refusal};
use crate::ui::{Abort, Prompter, Say};

/// What `pin` settled for a registry add's Ref.
#[derive(Debug, PartialEq, Eq)]
pub enum Pinned {
    /// The Version tag of the version given or read from the lockfile.
    Tag(String),
    /// A version was chosen but has no tag, or no tags could be listed: the remote's default
    /// branch, decided already.
    FollowsDefault,
    /// No version was given and the lockfile gave none (missing, unreadable, or without the
    /// Package). The caller may ask for the Ref.
    Unpinned,
}

/// What `package`, in the repo at `url`, is pinned to (see `Pinned`). With a `version` that is the one used; without, the project at `root` is
/// asked through its Package lockfile (`used::answer`). The tag for the
/// version is one of the `source`'s tags (`tag_for`); `--no-sync` has no `source`. A version
/// with no tag is never pinned silently: with a `prompter` the person is asked whether to follow
/// the default branch instead, otherwise it is a warning.
pub fn pin(
    package: &Package,
    version: Option<&str>,
    url: &str,
    root: &Path,
    source: Option<&dyn Source>,
    prompter: Option<&mut (dyn Prompter + '_)>,
    say: &mut dyn Say,
) -> Result<Pinned, Failure> {
    let mut pinning = Pinning {
        package,
        url,
        root,
        source,
        prompter,
        say,
    };
    match version {
        Some(version) => pinning.settle(version),
        None => pinning.read_lockfile(),
    }
}

const FOLLOWS: &str = "the repo follows the remote's default branch";
const NOT_PINNED: &str =
    "no version is pinned; the Ref is the remote's default branch unless one is given";
const HINT: &str = "use `--ref` to track a branch or choose another tag";

struct Pinning<'a, 'p> {
    package: &'a Package,
    url: &'a str,
    root: &'a Path,
    source: Option<&'a dyn Source>,
    prompter: Option<&'a mut (dyn Prompter + 'p)>,
    say: &'a mut dyn Say,
}

impl Pinning<'_, '_> {
    fn read_lockfile(&mut self) -> Result<Pinned, Failure> {
        let name = &self.package.name;
        match used::answer(self.package, self.root) {
            Answer::Missing => {
                self.say.status(format!(
                    "no Package lockfile found for `{name}`; {NOT_PINNED}"
                ));
                Ok(Pinned::Unpinned)
            }
            Answer::Unreadable { file, why } => {
                self.say
                    .warning(format!("cannot read {file}: {why}; {NOT_PINNED}"));
                Ok(Pinned::Unpinned)
            }
            Answer::Absent { file, ignored } => {
                self.ignored(&ignored, &file);
                self.say
                    .status(format!("`{name}` is not in {file}; {NOT_PINNED}"));
                Ok(Pinned::Unpinned)
            }
            Answer::One {
                file,
                version,
                ignored,
            } => {
                self.ignored(&ignored, &file);
                self.say.status(format!("`{name}` is {version} in {file}"));
                self.settle(&version)
            }
            Answer::Several {
                file,
                versions,
                ignored,
            } => {
                self.ignored(&ignored, &file);
                let Some(prompter) = self.prompter.as_deref_mut() else {
                    let why = several_versions_refusal(name, &versions, &file);
                    let name = name.clone();
                    return Err(AddError::SeveralVersions { why, name }.into());
                };
                let version = ask_version(prompter, name, &versions, &file);
                self.settle(&version.map_err(Failure::Asked)?)
            }
        }
    }

    /// Say which Package lockfiles were left unread in favour of `file`.
    fn ignored(&mut self, ignored: &[String], file: &str) {
        for ignored in ignored {
            self.say.status(format!(
                "{ignored} is also here and was not read; {file} was"
            ));
        }
    }

    /// Pins `version` to the tag it was released as, if the `source` lists one; otherwise follows
    /// the default branch (asked first when there is someone to ask).
    fn settle(&mut self, version: &str) -> Result<Pinned, Failure> {
        let name = &self.package.name;
        let Some(source) = self.source else {
            self.say.warning(format!(
                "`{name}@{version}` is not pinned to a tag, as `--no-sync` lists no tags; {FOLLOWS}; {HINT}"
            ));
            return Ok(Pinned::FollowsDefault);
        };
        let tags = source.tags(self.url)?;
        if let Some(tag) = tag_for(name, version, &tags) {
            self.say.status(format!(
                "pinned `{name}@{version}` to the tag `{tag}`; {HINT}"
            ));
            return Ok(Pinned::Tag(tag.to_owned()));
        }
        let untagged = format!("{name}@{version} is not tagged in {}", self.url);
        let Some(prompter) = self.prompter.as_deref_mut() else {
            self.say.warning(format!("{untagged}; {FOLLOWS}; {HINT}"));
            return Ok(Pinned::FollowsDefault);
        };
        let question = format!("{untagged}. Follow the remote's default branch instead?");
        match prompter.confirm(&question).map_err(Failure::Asked)? {
            true => {
                self.say.status(format!("{FOLLOWS}; {HINT}"));
                Ok(Pinned::FollowsDefault)
            }
            false => Err(Failure::Asked(Abort::Cancelled)),
        }
    }
}
