//! What `update` finds out about each Repo (ADR 0012), in two stages. `find` learns what a Repo
//! comes to from the remote and the lockfile alone: no question, no outline, so the bare-`update`
//! picker can label every Repo. `settle` runs only for a Repo named or picked: the dirty hold, the
//! version question, `mend`, `ask_ref`. See the module docs of `update` for what "now" means for
//! each kind of Ref.

use std::path::Path;

use super::Request;
use super::ask::fill_update;
// `update` shares the version question with `add`, so it imports from `add::pin` (and nothing else of `add`).
use crate::add::pin::choose_version;
use crate::config::{RepoRef, is_full_sha};
use crate::diagnostic::SourceError;
use crate::edit::Fields;
use crate::failure::Failure;
use crate::listing::{self, Gone};
use crate::lock::Lock;
use crate::manifests::Manifests;
use crate::registry::Package;
use crate::registry::tag::{newest, split, tag_for};
use crate::registry::used::{self, Answer};
use crate::source::{Observed, Outline, Source};
use crate::ui::{Abort, Prompter, Say};

/// A Repo that moves.
pub(super) struct Move {
    pub(super) id: String,
    /// The `ref` that `refs.toml` gets, and the Lock follows from the drift; none when only the
    /// Lock moves, to the remote's commit now.
    pub(super) git_ref: Option<String>,
    /// What the new tree is looked at by: the new Ref, else the commit the Lock moves to.
    look_at: String,
    /// `id  old → new`
    pub(super) line: String,
    /// The lists the new Ref made the person set again.
    pub(super) fields: Fields,
}

/// Where a Repo goes, once nothing is left to ask.
enum Outcome {
    Move(Move),
    /// Nothing to do: the reason, which reads after "`id` is" in a note and after the id in a label.
    Stay(String),
}

/// What is known about a Repo before anyone is asked anything.
enum Assessed {
    Ready(Outcome),
    /// The lockfile has several versions of the Package: which one moves it is for the person.
    Choose(Choice),
    /// The Ref is gone from the remote: another is for the person to give.
    Deleted,
    /// The Repo could not be assessed: why.
    Cannot(String),
}

impl Assessed {
    /// Whether the Repo would move, as opposed to one that stays or cannot be checked.
    fn would_move(&self) -> bool {
        matches!(
            self,
            Assessed::Ready(Outcome::Move(_)) | Assessed::Choose(_) | Assessed::Deleted
        )
    }
}

impl From<Outcome> for Assessed {
    fn from(outcome: Outcome) -> Self {
        Assessed::Ready(outcome)
    }
}

/// A Repo, what is known about it, and why its Checkout must not be moved over, if it moves.
pub(super) struct Candidate<'r> {
    pub(super) repo: RepoRef<'r>,
    found: Assessed,
    hold: Option<Hold>,
}

/// What it takes to move a pinned Repo once the version is chosen.
struct Choice {
    name: String,
    file: String,
    versions: Vec<String>,
    current: String,
    tags: Vec<String>,
}

/// The reason a Repo is left as it is, as a `Failure::Refused`.
fn skip(why: impl ToString) -> Failure {
    Failure::Refused(why.to_string())
}

/// What the stages of `update` share: where to look (`source`, `root`, the `lock`), who to ask and
/// where to say what was found.
pub(super) struct Assessing<'a, 'p> {
    source: &'a dyn Source,
    root: &'a Path,
    lock: Option<Lock>,
    prompter: Option<&'a mut (dyn Prompter + 'p)>,
    pub(super) say: &'a mut dyn Say,
    /// Move a Repo whose Checkout has uncommitted changes.
    force: bool,
}

impl<'a, 'p> Assessing<'a, 'p> {
    pub(super) fn new(
        source: &'a dyn Source,
        root: &'a Path,
        lock: Option<Lock>,
        prompter: Option<&'a mut (dyn Prompter + 'p)>,
        say: &'a mut dyn Say,
        force: bool,
    ) -> Self {
        Assessing {
            source,
            root,
            lock,
            prompter,
            say,
            force,
        }
    }

    /// What is known about `repo` from the remote and the lockfile alone: no question, no
    /// outline, no look at the Checkout.
    pub(super) fn find<'r>(&mut self, repo: RepoRef<'r>, request: &Request) -> Candidate<'r> {
        let found = match request.git_ref {
            Some(git_ref) => self.set(repo, git_ref).into(),
            None => self.follow(repo).unwrap_or_else(Assessed::Cannot),
        };
        let hold = found.would_move().then(|| self.hold_of(repo.id)).flatten();
        Candidate { repo, found, hold }
    }

    /// Decide what `candidate` comes to for a Repo the person named or picked: hold it back if its
    /// Checkout has changes, ask what it needs, then see what the new Ref breaks.
    pub(super) fn settle(&mut self, candidate: Candidate) -> Result<Option<Move>, Failure> {
        let repo = candidate.repo;
        if let Some(hold) = candidate.hold.filter(|_| !self.force) {
            return Err(skip(format!(
                "its checkout has {}; `--force` moves it anyway",
                hold.why()
            )));
        }
        let outcome = match candidate.found {
            Assessed::Ready(outcome) => outcome,
            Assessed::Choose(choice) => self.choose(repo, choice)?,
            Assessed::Deleted => self.ask_ref(repo)?,
            Assessed::Cannot(why) => return Err(skip(why)),
        };
        match outcome {
            Outcome::Move(found) => self.mend(repo, found),
            Outcome::Stay(reason) => Ok(self.stays(repo.id, &reason)),
        }
    }

    /// Say `id` stays, and why.
    fn stays(&mut self, id: &str, reason: &str) -> Option<Move> {
        self.say.status(format!("`{id}` is {reason}"));
        None
    }

    /// The multi-select over `found`, nothing ticked: the ids of the Repos picked.
    pub(super) fn pick<'r>(&mut self, found: &[Candidate<'r>]) -> Result<Vec<&'r str>, Abort> {
        let labels: Vec<String> = found
            .iter()
            .map(|candidate| {
                let dirty = matches!(candidate.hold, Some(Hold::Dirty(_)));
                let held = match !self.force && dirty {
                    true => "  (uncommitted changes)",
                    false => "",
                };
                format!("{}{held}", self.label(candidate))
            })
            .collect();
        let Some(prompter) = self.prompter.as_deref_mut() else {
            return Ok(vec![]);
        };
        let picked = prompter.pick("Update", &labels, &[])?;
        Ok(found
            .iter()
            .zip(&labels)
            .filter(|(_, label)| picked.contains(label))
            .map(|(candidate, _)| candidate.repo.id)
            .collect())
    }

    /// `id  current → new`, or why there is none.
    fn label(&self, candidate: &Candidate) -> String {
        let id = candidate.repo.id;
        match &candidate.found {
            Assessed::Ready(Outcome::Move(found)) => found.line.clone(),
            Assessed::Choose(choice) => format!("{id}  {} → choose version…", choice.current),
            Assessed::Deleted => format!("{id}  (its Ref is no longer in the remote)"),
            Assessed::Ready(Outcome::Stay(reason)) => format!("{id}  {reason}"),
            Assessed::Cannot(why) => format!("{id}  (cannot check: {why})"),
        }
    }

    /// `--list`: for each Repo its label, then a line for what the move would break and for why
    /// it would be skipped. Nothing is asked.
    pub(super) fn listing(&self, found: &[Candidate]) -> String {
        let mut text = String::new();
        for candidate in found {
            text += &format!("{}\n", self.label(candidate));
            let mut notes = vec![];
            match &candidate.found {
                Assessed::Ready(Outcome::Move(found)) => match self.look(candidate.repo, found) {
                    Ok((_, gone)) if gone.is_empty() => {}
                    Ok((_, gone)) => notes.push(format!("no longer has {gone}")),
                    Err(e) => notes.push(format!("cannot check: {e}")),
                },
                Assessed::Choose(choice) => {
                    let versions = choice.versions.join(", ");
                    notes.push(format!("{} has {versions}", choice.file));
                }
                _ => {}
            }
            if let Some(hold) = &candidate.hold {
                notes.push(format!("would be skipped: {}", hold.why()));
            }
            for note in notes {
                text += &format!("  {note}\n");
            }
        }
        text
    }

    /// Outline where `found` goes. If that tree lacks a `path` or `start` file of the Repo, say
    /// so and ask `Update?`: yes sets the broken fields again, no leaves the Repo as it is.
    fn mend(&mut self, repo: RepoRef, mut found: Move) -> Result<Option<Move>, Failure> {
        let (id, url) = (repo.id, repo.repo.url.as_ref());
        let to = found
            .git_ref
            .as_deref()
            .unwrap_or(repo.repo.effective_ref());
        let (outline, gone) = self.look(repo, &found).map_err(skip)?;
        let paths = repo.repo.path_strings();
        if gone.is_empty() {
            return Ok(Some(found));
        }
        let Some(prompter) = self.prompter.as_deref_mut() else {
            return Err(skip(format!(
                "`{to}` no longer has {gone}; run `refs update {id}` in a terminal to choose others"
            )));
        };
        let manifests = Manifests::read(self.source, url, &outline, &[], self.say).map_err(skip)?;
        let have = manifests.packages_for(&paths);
        let vanished: Vec<String> = (repo.repo.package_strings().into_iter())
            .filter(|name| !have.contains(name))
            .collect();
        self.say
            .shown(format!("`{id}`: `{to}` no longer has {gone}"));
        if !vanished.is_empty() {
            let names: Vec<_> = vanished.iter().map(|name| format!("`{name}`")).collect();
            self.say.warning(format!(
                "`{id}`: `{to}` has no package {}; kept",
                names.join(", ")
            ));
        }
        let asked = fill_update(
            repo.repo,
            &outline.paths,
            &gone,
            &manifests,
            prompter,
            self.say,
        );
        match asked.map_err(Failure::Asked)? {
            Some(fields) => {
                found.fields = fields;
                Ok(Some(found))
            }
            None => Ok(self.stays(id, &format!("left at `{}`", repo.repo.effective_ref()))),
        }
    }

    /// Outline where `found` goes, and what that tree lacks of the Repo's `paths` and `start`.
    fn look(&self, repo: RepoRef, found: &Move) -> Result<(Outline, Gone), SourceError> {
        let outline = self
            .source
            .outline(repo.repo.url.as_ref(), &found.look_at)?;
        let gone = listing::gone(
            &outline.paths,
            &repo.repo.path_strings(),
            &repo.repo.start_strings(),
        );
        Ok((outline, gone))
    }

    /// `--ref`: the Ref asked for, if the remote has it.
    fn set(&self, repo: RepoRef, git_ref: &str) -> Outcome {
        let id = repo.id;
        let current = repo.repo.effective_ref();
        if git_ref == current {
            return Outcome::Stay(format!("already at `{current}`"));
        }
        moved_to(id, current, git_ref)
    }

    /// Where the news about `repo` comes from depends on its Ref (see the module docs).
    fn follow(&mut self, repo: RepoRef) -> Result<Assessed, String> {
        let git_ref = repo.repo.configured_ref();
        if git_ref.is_some_and(is_full_sha) {
            return Ok(Outcome::Stay("pinned to a commit; use `--ref` to move it".into()).into());
        }
        let Some(git_ref) = git_ref else {
            return self.by_remote(repo);
        };
        let tags = self
            .source
            .tags(repo.repo.url.as_ref())
            .map_err(|e| e.to_string())?;
        if !tags.iter().any(|tag| tag == git_ref) {
            return self.by_remote(repo);
        }
        let Some(origin) = &repo.repo.origin else {
            return Ok(by_kind(repo, git_ref, &tags).into());
        };
        let package = Package::from_origin(origin.as_ref())
            .expect("an origin is validated when the config loads");
        self.by_lockfile(repo, &package, git_ref, &tags)
    }

    /// The Version tag of the version the project's Package lockfile has for `package`.
    fn by_lockfile(
        &mut self,
        repo: RepoRef,
        package: &Package,
        current: &str,
        tags: &[String],
    ) -> Result<Assessed, String> {
        let name = &package.name;
        let version = match used::answer(package, self.root) {
            Answer::Missing | Answer::Absent { .. } => {
                return Ok(by_kind(repo, current, tags).into());
            }
            Answer::Unreadable { file, why } => return Err(format!("cannot read {file}: {why}")),
            Answer::One { version, .. } => version,
            Answer::Several { file, versions, .. } => {
                return Ok(Assessed::Choose(Choice {
                    name: name.clone(),
                    file,
                    versions,
                    current: current.into(),
                    tags: tags.to_vec(),
                }));
            }
        };
        Ok(by_version(repo, name, &version, current, tags).into())
    }

    /// Ask which of the versions the lockfile has moves `repo`.
    fn choose(&mut self, repo: RepoRef, choice: Choice) -> Result<Outcome, Failure> {
        let Choice {
            name,
            file,
            versions,
            current,
            tags,
        } = choice;
        let list: Vec<&str> = versions.iter().map(String::as_str).collect();
        let version = choose_version(
            self.prompter.as_deref_mut(),
            &name,
            &list,
            &file,
            &format!("use `refs update {} --ref <tag>` to choose one", repo.id),
        )?;
        Ok(by_version(repo, &name, &version, &current, &tags))
    }

    /// A branch Ref moves when the remote's commit is not the Lock's.
    fn by_remote(&mut self, repo: RepoRef) -> Result<Assessed, String> {
        let id = repo.id;
        let Some(locked) = self.lock.as_ref().and_then(|lock| lock.get(id)) else {
            return Ok(Outcome::Stay("not locked yet; `refs sync` locks it".into()).into());
        };
        let now = match self.source.resolve(repo) {
            Err(SourceError::RefNotFound { .. }) => return Ok(Assessed::Deleted),
            now => now.map_err(|e| e.to_string())?,
        };
        if now.sha() == locked.pin.sha() {
            return Ok(Outcome::Stay("up to date".into()).into());
        }
        Ok(Outcome::Move(Move {
            id: id.into(),
            git_ref: None,
            look_at: now.sha().into(),
            line: format!(
                "{id}  {} → {} ({})",
                locked.pin.short_id(),
                now.short_id(),
                repo.repo.effective_ref()
            ),
            fields: Fields::default(),
        })
        .into())
    }

    /// The Ref of `repo` is gone from the remote: ask for another, starting from the old one.
    fn ask_ref(&mut self, repo: RepoRef) -> Result<Outcome, Failure> {
        let (id, old, url) = (repo.id, repo.repo.effective_ref(), repo.repo.url.as_ref());
        let Some(prompter) = self.prompter.as_deref_mut() else {
            return Err(skip(format!(
                "`{old}` is no longer in {url}; use `refs update {id} --ref <ref>`"
            )));
        };
        self.say
            .shown(format!("`{id}`: `{old}` is no longer in {url}"));
        let valid = |new: &str| match new.trim().is_empty() {
            true => Err("a Ref is needed".to_owned()),
            false => Ok(()),
        };
        let new = prompter
            .text("Ref", Some(old), &valid)
            .map_err(Failure::Asked)?;
        Ok(moved_to(id, old, new.trim()))
    }

    /// Why the Checkout of `id` must not be moved over, if it must not.
    fn hold_of(&self, id: &str) -> Option<Hold> {
        match self.source.inspect(id) {
            Ok(Observed::At { dirty_files, .. }) if !dirty_files.is_empty() => {
                Some(Hold::Dirty(dirty_files))
            }
            Err(e) => Some(Hold::Uninspectable(e.to_string())),
            _ => None,
        }
    }
}

/// Why a Checkout is not moved over.
enum Hold {
    Dirty(Vec<String>),
    Uninspectable(String),
}

impl Hold {
    /// What the Checkout has, as it reads after "its checkout has".
    fn why(&self) -> String {
        match self {
            Hold::Dirty(files) => format!("uncommitted changes ({})", files.join(", ")),
            Hold::Uninspectable(e) => format!("a state that cannot be inspected: {e}"),
        }
    }
}

/// Where `repo` moves from the tag `current` when no Package lockfile answers: the newest tag of
/// its own kind (`tag::newest`). A tag with no version stays, with a note pointing at `--ref`.
fn by_kind(repo: RepoRef, current: &str, tags: &[String]) -> Outcome {
    if split(current).is_none() {
        return Outcome::Stay(format!(
            "not moved: `{current}` has no version to compare; use `--ref` to move it"
        ));
    }
    match newest(current, tags) {
        Some(tag) => moved_to(repo.id, current, tag),
        None => Outcome::Stay("up to date".into()),
    }
}

/// Where the lockfile's `version` of the Package moves `repo` from `current`, by its Version tag.
fn by_version(repo: RepoRef, name: &str, version: &str, current: &str, tags: &[String]) -> Outcome {
    let id = repo.id;
    match tag_for(name, version, tags) {
        Some(tag) if tag == current => Outcome::Stay("up to date".into()),
        Some(tag) => moved_to(id, current, tag),
        None => Outcome::Stay(format!(
            "not moved: {name}@{version} is not tagged in {}; use `--ref` to choose a tag",
            repo.repo.url.as_ref()
        )),
    }
}

fn moved_to(id: &str, from: &str, to: &str) -> Outcome {
    Outcome::Move(Move {
        id: id.into(),
        git_ref: Some(to.into()),
        look_at: to.into(),
        line: format!("{id}  {from} → {to}"),
        fields: Fields::default(),
    })
}
