//! What `add` pins a registry package to (spec §7.2): the version given or the one a Package
//! lockfile says the project uses, and the Version tag it was released as. It decides, says what
//! it read and decided as it goes (`Say`, so a line comes before the question it leads to), and
//! asks the two questions that can come up through the `Prompter`: which of several versions,
//! and whether a version with no tag may follow the default branch.

use std::path::Path;

use super::Package;
use super::tag::tag_for;
use super::used::{self, Lookup, Pick};
use crate::diagnostic::SourceError;
use crate::prompt::{Abort, Prompter};
use crate::source::Source;

/// Where `pin` says what it read and decided.
pub trait Say {
    /// What was read or decided.
    fn status(&mut self, line: String);
    /// Something the person should see even when the run is quiet.
    fn warning(&mut self, line: String);
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

/// The tag that `package`, in the repo at `url`, is pinned to, or `None` to follow the remote's
/// default branch. With a `version` that is the one used; without, the project at `root` is
/// asked through its Package lockfile (`used::find`, then `Found::pick`). The tag for the
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
) -> Result<Option<String>, Stop> {
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

struct Pinning<'a, 'p> {
    package: &'a Package,
    url: &'a str,
    root: &'a Path,
    source: Option<&'a dyn Source>,
    prompter: Option<&'a mut (dyn Prompter + 'p)>,
    say: &'a mut dyn Say,
}

impl Pinning<'_, '_> {
    /// `file` as it is named in a message: relative to the project if it is below it.
    fn name(&self, file: &Path) -> String {
        let file = file.strip_prefix(self.root).unwrap_or(file);
        file.display().to_string()
    }

    fn read_lockfile(&mut self) -> Result<Option<String>, Stop> {
        let name = &self.package.name;
        let found = match used::find(self.package, self.root) {
            Lookup::Found(found) => found,
            Lookup::Missing => {
                self.say
                    .status(format!("no Package lockfile found for `{name}`; {FOLLOWS}"));
                return Ok(None);
            }
            Lookup::Unreadable { file, why } => {
                let file = self.name(&file);
                self.say
                    .warning(format!("cannot read {file}: {why}; {FOLLOWS}"));
                return Ok(None);
            }
        };
        let file = self.name(&found.file);
        for ignored in &found.ignored {
            let ignored = self.name(ignored);
            self.say.status(format!(
                "{ignored} is also here and was not read; {file} was"
            ));
        }
        match found.pick() {
            Pick::Nothing => {
                self.say
                    .status(format!("`{name}` is not in {file}; {FOLLOWS}"));
                Ok(None)
            }
            Pick::One(version) => {
                self.say.status(format!("`{name}` is {version} in {file}"));
                self.settle(version)
            }
            Pick::Several(versions) => {
                let version = self.choose(&versions, &file)?;
                self.settle(&version)
            }
        }
    }

    /// Which of `versions` to use: asked, with the highest as the default, or refused without
    /// someone to ask.
    fn choose(&mut self, versions: &[&str], file: &str) -> Result<String, Stop> {
        let name = &self.package.name;
        let list = versions.join(", ");
        let Some(prompter) = self.prompter.as_deref_mut() else {
            return Err(Stop::Refused(format!(
                "{file} has several versions of `{name}` ({list}), and none is the one the \
                 project asks for; give one as `{name}@version`"
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

    /// The tag `version` was released as, if the `source` lists one.
    fn settle(&mut self, version: &str) -> Result<Option<String>, Stop> {
        let name = &self.package.name;
        let Some(source) = self.source else {
            self.say.warning(format!(
                "`{name}@{version}` is not pinned to a tag, as `--no-sync` lists no tags; {FOLLOWS}"
            ));
            return Ok(None);
        };
        let tags = source.tags(self.url).map_err(Stop::Source)?;
        if let Some(tag) = tag_for(name, version, &tags) {
            self.say
                .status(format!("pinned `{name}@{version}` to the tag `{tag}`"));
            return Ok(Some(tag.to_owned()));
        }
        let untagged = format!("{name}@{version} is not tagged in {}", self.url);
        let Some(prompter) = self.prompter.as_deref_mut() else {
            self.say.warning(format!("{untagged}; {FOLLOWS}"));
            return Ok(None);
        };
        let question = format!("{untagged}. Follow the remote's default branch instead?");
        match prompter.confirm(&question).map_err(Stop::Asked)? {
            true => Ok(None),
            false => Err(Stop::Asked(Abort::Cancelled)),
        }
    }
}
