//! What `add` pins a registry package to (spec §7.2): the version given or the one a Package
//! lockfile says the project uses, and the Version tag it was released as. It decides, says what
//! it read and decided as it goes (`Say`, so a line comes before the question it leads to), and
//! asks the two questions that can come up through the `Prompter`: which of several versions,
//! and whether a version with no tag may follow the default branch.

use std::path::Path;

use super::Package;
use super::tag::tag_for;
use super::used::{self, Answer};
use crate::diagnostic::SourceError;
use crate::prompt::{Abort, Prompter};
use crate::source::Source;

/// Where `pin` says what it read and decided.
pub trait Say {
    /// What was read or decided.
    fn status(&mut self, line: String);
    /// Something the person should see even when the run is quiet.
    fn warning(&mut self, line: String);
    /// What a question is about: shown even when the run is quiet.
    fn shown(&mut self, line: String);
}

/// Why `pin` could not say what to pin to.
#[derive(Debug)]
pub enum Stop {
    /// The tags of the remote could not be listed.
    Source(SourceError),
    /// A question ended without an answer.
    Asked(Abort),
    /// A question had to be asked and there is no one to ask: the message says what to do.
    Refused(String),
}

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
) -> Result<Pinned, Stop> {
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

/// Which of `versions` of `name` to use: asked, with the highest as the default, or refused
/// without someone to ask; the refusal ends with `instead`, what to do.
pub fn choose_version(
    prompter: Option<&mut (dyn Prompter + '_)>,
    name: &str,
    versions: &[&str],
    file: &str,
    instead: &str,
) -> Result<String, Stop> {
    let list = versions.join(", ");
    let Some(prompter) = prompter else {
        return Err(Stop::Refused(format!(
            "{file} has several versions of `{name}` ({list}), and none is the one the \
             project asks for; {instead}"
        )));
    };
    let one_of_them = |answer: &str| match versions.contains(&answer) {
        true => Ok(()),
        false => Err(format!("one of {list}")),
    };
    let message = format!("{file} has several versions of `{name}`: {list}. Which one?");
    prompter
        .text(&message, versions.last().copied(), &one_of_them)
        .map_err(Stop::Asked)
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
    fn read_lockfile(&mut self) -> Result<Pinned, Stop> {
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
                let versions: Vec<&str> = versions.iter().map(String::as_str).collect();
                let version = choose_version(
                    self.prompter.as_deref_mut(),
                    name,
                    &versions,
                    &file,
                    &format!("give one as `{name}@version`"),
                )?;
                self.settle(&version)
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
    fn settle(&mut self, version: &str) -> Result<Pinned, Stop> {
        let name = &self.package.name;
        let Some(source) = self.source else {
            self.say.warning(format!(
                "`{name}@{version}` is not pinned to a tag, as `--no-sync` lists no tags; {FOLLOWS}; {HINT}"
            ));
            return Ok(Pinned::FollowsDefault);
        };
        let tags = source.tags(self.url).map_err(Stop::Source)?;
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
        match prompter.confirm(&question).map_err(Stop::Asked)? {
            true => {
                self.say.status(format!("{FOLLOWS}; {HINT}"));
                Ok(Pinned::FollowsDefault)
            }
            false => Err(Stop::Asked(Abort::Cancelled)),
        }
    }
}
