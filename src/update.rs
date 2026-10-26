//! `refs update <id…>` (ADR 0012): move the named Repos to where their Ref should be now, in one
//! edit and one sync. What "now" means depends on the Ref. A tag Ref of a Repo with an Origin
//! follows the version the project's Package lockfile says it uses, through its Version tag; when
//! no lockfile answers (no Origin, no lockfile, or the Package is not in it) it moves to the
//! newest tag of its own kind (`tag::newest`). A branch Ref (no Ref, or one that is not a tag)
//! follows the remote, and moves when the remote's commit differs from the Lock's; a commit has
//! no news and a note pointing at `--ref`. `--ref` sets the Ref by hand. The Registry is never asked.
//! `--list` stops after that assessment and prints what each Repo would do and break.
//!
//! Everything that can be found out before the edit is: a Repo that cannot be assessed, whose
//! new Ref does not exist or whose Checkout has uncommitted changes (unless forced) is skipped
//! and reported, and the rest go through. What the remote does after the edit (stage 1 resolves
//! and verifies the whole edited config) blocks the run, as it does for any edit.

use std::path::Path;

use crate::active::active;
use crate::config::{Config, RepoRef, is_full_sha};
use crate::diagnostic::LockError;
use crate::diagnostic::SourceError;
use crate::edit::{Edit, Fields, RepoChange};
use crate::listing::{self, Gone};
use crate::lock::Lock;
use crate::packages::{self, manifest_paths};
use crate::plan::Upgrade;
use crate::prompt::{Abort, Broken, Prompter, fill_update};
use crate::registry::Package;
use crate::registry::pin::{Say, Stop, choose_version};
use crate::registry::tag::{newest, split, tag_for};
use crate::registry::used::{self, Lookup, Pick};
use crate::source::{Observed, Outline, Source};
use crate::sync::{self, Report, SyncFlags};

/// What `refs update` was asked for.
pub struct Request<'a> {
    pub ids: &'a [String],
    /// `--ref`: the Ref to set, for the one Repo named.
    pub git_ref: Option<&'a str>,
    /// Move a Repo whose Checkout has uncommitted changes, discarding them.
    pub force: bool,
    /// `--list`: say what would move and what it would break; ask nothing, change nothing.
    pub list: bool,
    pub offline: bool,
}

/// Why `update` stopped before it edited anything.
#[derive(Debug)]
pub enum Failure {
    /// The command line asked for something that cannot be done: the message says what.
    Refused(String),
    /// The command line is wrong.
    Usage(String),
    Lock(LockError),
    /// A question ended without an answer.
    Asked(Abort),
}

/// What a run came to.
pub struct Done {
    /// The sync of the edit; none when no Repo moved.
    pub report: Option<Report>,
    /// Repos that could not be assessed or moved, each reported already.
    pub skipped: usize,
    /// `--list`: the lines to print, one block per Repo.
    pub listing: String,
}

/// Update the Repos `request` names. What it found out and decided is said through `say`: notes
/// as status, skips as problems, and one `id  old → new` line per Repo once the edit was written.
pub fn run(
    source: &dyn Source,
    root: &Path,
    config: &Config,
    request: &Request,
    prompter: Option<&mut dyn Prompter>,
    say: &mut dyn Say,
) -> Result<Done, Failure> {
    if request.offline {
        return Err(Failure::Refused(
            "`refs update` asks the remotes and needs the network; drop `--offline`".into(),
        ));
    }
    if request.git_ref.is_some() && request.ids.len() != 1 {
        return Err(Failure::Usage("`--ref` sets the Ref of one repo".into()));
    }
    let active = active(config);
    let unknown: Vec<String> = request
        .ids
        .iter()
        .filter(|id| active.get(id).is_none())
        .map(|id| format!("`{id}`"))
        .collect();
    if !unknown.is_empty() {
        return Err(Failure::Refused(format!(
            "no enabled repo named {}",
            unknown.join(", ")
        )));
    }
    let ids: Vec<&str> = match request.ids {
        [] => match prompter {
            Some(_) => active.repos().map(|repo| repo.id).collect(),
            None if request.list => active.repos().map(|repo| repo.id).collect(),
            None => {
                return Err(Failure::Usage(
                    "`refs update` without repo names needs a terminal; name the repos or use `--list`"
                        .into(),
                ));
            }
        },
        named => named.iter().map(String::as_str).collect(),
    };
    let lock = Lock::read(&Lock::path(root)).map_err(Failure::Lock)?;
    let mut assessing = Assessing {
        source,
        root,
        lock,
        prompter,
        say: &mut *say,
        force: request.force,
    };
    let mut seen = Vec::<&str>::new();
    let mut found = Vec::new();
    if (request.ids.is_empty() || request.list) && !ids.is_empty() {
        assessing
            .say
            .status(format!("checking {} repos…", ids.len()));
    }
    for id in ids {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let repo = active.get(id).expect("the ids were checked");
        found.push(assessing.find(repo, request));
    }
    if request.list {
        return Ok(Done {
            report: None,
            skipped: 0,
            listing: assessing.listing(&found),
        });
    }
    let chosen = match request.ids.is_empty() {
        true => assessing.pick(&found).map_err(Failure::Asked)?,
        false => seen.clone(),
    };
    let (mut moves, mut skipped) = (Vec::<Move>::new(), 0);
    for candidate in found {
        let id = candidate.repo.id;
        if !chosen.contains(&id) {
            continue;
        }
        match assessing.settle(candidate) {
            Ok(moved) => moves.extend(moved),
            Err(stop) => {
                let why = match stop {
                    Stop::Asked(abort) => return Err(Failure::Asked(abort)),
                    Stop::Source(e) => e.to_string(),
                    Stop::Refused(why) => why,
                };
                assessing
                    .say
                    .shown(format!("`{id}` was not updated: {why}"));
                skipped += 1;
            }
        }
    }
    if moves.is_empty() {
        return Ok(Done {
            report: None,
            skipped,
            listing: String::new(),
        });
    }
    Ok(apply(source, root, moves, skipped, request.force, say))
}

/// Edit once for all `moves`, update the Lock, sync, and say `id  old → new` for each.
fn apply(
    source: &dyn Source,
    root: &Path,
    moves: Vec<Move>,
    skipped: usize,
    force: bool,
    say: &mut dyn Say,
) -> Done {
    let changes: Vec<RepoChange> = moves
        .iter()
        .map(|found| RepoChange {
            id: &found.id,
            git_ref: found.git_ref.as_deref(),
            fields: found.fields.clone(),
        })
        .collect();
    let upgrade: Vec<String> = moves
        .iter()
        .filter(|found| found.git_ref.is_none())
        .map(|found| found.id.clone())
        .collect();
    let flags = SyncFlags {
        force,
        upgrade: if upgrade.is_empty() {
            Upgrade::None
        } else {
            Upgrade::Ids(upgrade)
        },
        ..SyncFlags::default()
    };
    let mut report = sync::edit(source, root, &Edit::Change(&changes), &flags);
    if skipped > 0 {
        // The run ends with the count of skipped Repos, not the hint to fix a broken one.
        report.hint = None;
    }
    if !report.changes.is_empty() {
        for found in moves {
            say.status(found.line);
        }
    }
    Done {
        report: Some(report),
        skipped,
        listing: String::new(),
    }
}

/// A Repo that moves.
struct Move {
    id: String,
    /// The `ref` that `refs.toml` gets, and the Lock follows from the drift; none when only the
    /// Lock moves, to the remote's commit now.
    git_ref: Option<String>,
    /// What the new tree is looked at by: the new Ref, else the commit the Lock moves to.
    look_at: String,
    /// `id  old → new`
    line: String,
    /// The lists the new Ref made the person set again.
    fields: Fields,
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
struct Candidate<'r> {
    repo: RepoRef<'r>,
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

/// The reason a Repo is left as it is, as a `Stop` that is `Refused`.
fn skip(why: impl ToString) -> Stop {
    Stop::Refused(why.to_string())
}

struct Assessing<'a, 'p> {
    source: &'a dyn Source,
    root: &'a Path,
    lock: Option<Lock>,
    prompter: Option<&'a mut (dyn Prompter + 'p)>,
    say: &'a mut dyn Say,
    /// Move a Repo whose Checkout has uncommitted changes.
    force: bool,
}

impl Assessing<'_, '_> {
    /// What is known about `repo` from the remote and the lockfile alone: no question, no
    /// outline, no look at the Checkout.
    fn find<'r>(&mut self, repo: RepoRef<'r>, request: &Request) -> Candidate<'r> {
        let found = match request.git_ref {
            Some(git_ref) => self.set(repo, git_ref).into(),
            None => self.follow(repo).unwrap_or_else(Assessed::Cannot),
        };
        let hold = found.would_move().then(|| self.hold_of(repo.id)).flatten();
        Candidate { repo, found, hold }
    }

    /// Decide what `candidate` comes to for a Repo the person named or picked: hold it back if its
    /// Checkout has changes, ask what it needs, then see what the new Ref breaks.
    fn settle(&mut self, candidate: Candidate) -> Result<Option<Move>, Stop> {
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
    fn pick<'r>(&mut self, found: &[Candidate<'r>]) -> Result<Vec<&'r str>, Abort> {
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
    fn listing(&self, found: &[Candidate]) -> String {
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
    fn mend(&mut self, repo: RepoRef, mut found: Move) -> Result<Option<Move>, Stop> {
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
        let (manifests, _) = manifest_paths(&outline.paths, &[]);
        let manifests = self
            .source
            .read(url, &outline.pin, &manifests)
            .map_err(skip)?;
        let packages_for = |paths: &[String]| packages::packages_for(&manifests, paths);
        let have = packages_for(&paths);
        let vanished: Vec<String> = (repo.repo.package_strings().into_iter())
            .filter(|name| !have.contains(name))
            .collect();
        let broken = Broken {
            id,
            to,
            repo: repo.repo,
            listed: &outline.paths,
            gone: &gone,
            vanished: &vanished,
            packages_for: &packages_for,
        };
        match fill_update(&broken, prompter, self.say).map_err(Stop::Asked)? {
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
        let found = match used::find(package, self.root) {
            Lookup::Found(found) => found,
            Lookup::Missing => return Ok(by_kind(repo, current, tags).into()),
            Lookup::Unreadable { file, why } => {
                return Err(format!("cannot read {}: {why}", file.display()));
            }
        };
        let file = found
            .file
            .strip_prefix(self.root)
            .unwrap_or(&found.file)
            .display()
            .to_string();
        let version = match found.pick() {
            Pick::Nothing => return Ok(by_kind(repo, current, tags).into()),
            Pick::One(version) => version.to_owned(),
            Pick::Several(versions) => {
                return Ok(Assessed::Choose(Choice {
                    name: name.clone(),
                    file,
                    versions: versions.iter().map(|&v| v.to_owned()).collect(),
                    current: current.into(),
                    tags: tags.to_vec(),
                }));
            }
        };
        Ok(by_version(repo, name, &version, current, tags).into())
    }

    /// Ask which of the versions the lockfile has moves `repo`.
    fn choose(&mut self, repo: RepoRef, choice: Choice) -> Result<Outcome, Stop> {
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
    fn ask_ref(&mut self, repo: RepoRef) -> Result<Outcome, Stop> {
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
            .map_err(Stop::Asked)?;
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
