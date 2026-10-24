//! `refs update <id…>` (ADR 0012): move the named Repos to where their Ref should be now, in one
//! edit and one sync. What "now" means depends on the Ref. A tag Ref of a Repo with an Origin
//! follows the version the project's Package lockfile says it uses, through its Version tag; a
//! branch Ref (no Ref, or one that is not a tag) follows the remote, and moves when the remote's
//! commit differs from the Lock's; a commit, or a tag with no Origin, has no news and a note
//! pointing at `--ref`. `--ref` sets the Ref by hand. The Registry is never asked.
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
use crate::listing;
use crate::lock::Lock;
use crate::packages::{self, manifest_paths};
use crate::plan::Upgrade;
use crate::prompt::{Abort, Broken, Prompter, fill_update};
use crate::registry::Package;
use crate::registry::pin::{Say, Stop, choose_version};
use crate::registry::tag::tag_for;
use crate::registry::used::{self, Lookup, Pick};
use crate::source::{Observed, Source};
use crate::sync::{self, Report, SyncFlags};

/// What `refs update` was asked for.
pub struct Request<'a> {
    pub ids: &'a [String],
    /// `--ref`: the Ref to set, for the one Repo named.
    pub git_ref: Option<&'a str>,
    /// Move a Repo whose Checkout has uncommitted changes, discarding them.
    pub force: bool,
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
    let lock = Lock::read(&Lock::path(root)).map_err(Failure::Lock)?;
    let mut assessing = Assessing {
        source,
        root,
        lock,
        prompter,
        say: &mut *say,
        force: request.force,
    };
    let (mut moves, mut skipped) = (Vec::<Move>::new(), 0);
    let mut seen = Vec::<&str>::new();
    for id in request.ids.iter().map(String::as_str) {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let repo = active.get(id).expect("the ids were checked");
        match assessing.assess(repo, request) {
            Ok(Assessed::Move(found)) => moves.push(found),
            Ok(Assessed::Stay(note)) => assessing.say.status(note),
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
        });
    }
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
        force: request.force,
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
    Ok(Done {
        report: Some(report),
        skipped,
    })
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

enum Assessed {
    Move(Move),
    /// Nothing to do, and the note says why.
    Stay(String),
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
    fn assess(&mut self, repo: RepoRef, request: &Request) -> Result<Assessed, Stop> {
        let found = match request.git_ref {
            Some(git_ref) => self.set(repo, git_ref)?,
            None => self.follow(repo)?,
        };
        let Assessed::Move(found) = found else {
            return Ok(found);
        };
        self.must_be_clean(repo.id)?;
        self.mend(repo, found)
    }

    /// Outline where `found` goes. If that tree lacks a `path` or `start` file of the Repo, say
    /// so and ask `Update?`: yes sets the broken fields again, no leaves the Repo as it is.
    fn mend(&mut self, repo: RepoRef, mut found: Move) -> Result<Assessed, Stop> {
        let (id, url) = (repo.id, repo.repo.url.as_ref());
        let to = found
            .git_ref
            .as_deref()
            .unwrap_or(repo.repo.effective_ref());
        let outline = self.source.outline(url, &found.look_at).map_err(skip)?;
        let (paths, start) = (repo.repo.path_strings(), repo.repo.start_strings());
        let gone = listing::gone(&outline.paths, &paths, &start);
        if gone.is_empty() {
            return Ok(Assessed::Move(found));
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
                Ok(Assessed::Move(found))
            }
            None => Ok(Assessed::Stay(format!(
                "`{id}` is left at `{}`",
                repo.repo.effective_ref()
            ))),
        }
    }

    /// `--ref`: the Ref asked for, if the remote has it.
    fn set(&self, repo: RepoRef, git_ref: &str) -> Result<Assessed, Stop> {
        let id = repo.id;
        let current = repo.repo.effective_ref();
        if git_ref == current {
            return Ok(Assessed::Stay(format!("`{id}` is already at `{current}`")));
        }
        Ok(moved_to(id, current, git_ref))
    }

    /// Where the news about `repo` comes from depends on its Ref (see the module docs).
    fn follow(&mut self, repo: RepoRef) -> Result<Assessed, Stop> {
        let id = repo.id;
        let git_ref = repo.repo.configured_ref();
        if git_ref.is_some_and(is_full_sha) {
            return Ok(Assessed::Stay(format!(
                "`{id}` is pinned to a commit; use `--ref` to move it"
            )));
        }
        let Some(git_ref) = git_ref else {
            return self.by_remote(repo);
        };
        let tags = self.source.tags(repo.repo.url.as_ref()).map_err(skip)?;
        if !tags.iter().any(|tag| tag == git_ref) {
            return self.by_remote(repo);
        }
        let Some(origin) = &repo.repo.origin else {
            return Ok(Assessed::Stay(format!(
                "`{id}` is pinned and has no origin; use `--ref` to move it"
            )));
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
    ) -> Result<Assessed, Stop> {
        let (id, name) = (repo.id, &package.name);
        let up_to_date = |why: String| Ok(Assessed::Stay(format!("`{id}` is up to date ({why})")));
        let found = match used::find(package, self.root) {
            Lookup::Found(found) => found,
            Lookup::Missing => {
                return up_to_date(format!("no Package lockfile found for `{name}`"));
            }
            Lookup::Unreadable { file, why } => {
                return Err(skip(format!("cannot read {}: {why}", file.display())));
            }
        };
        let file = found
            .file
            .strip_prefix(self.root)
            .unwrap_or(&found.file)
            .display()
            .to_string();
        let version = match found.pick() {
            Pick::Nothing => return up_to_date(format!("`{name}` is not in {file}")),
            Pick::One(version) => version.to_owned(),
            Pick::Several(versions) => {
                self.must_be_clean(id)?;
                choose_version(
                    self.prompter.as_deref_mut(),
                    name,
                    &versions,
                    &file,
                    &format!("use `refs update {id} --ref <tag>` to choose one"),
                )?
            }
        };
        match tag_for(name, &version, tags) {
            Some(tag) if tag == current => Ok(Assessed::Stay(format!("`{id}` is up to date"))),
            Some(tag) => Ok(moved_to(id, current, tag)),
            None => Ok(Assessed::Stay(format!(
                "`{id}` is not moved: {name}@{version} is not tagged in {}; \
                 use `--ref` to choose a tag",
                repo.repo.url.as_ref()
            ))),
        }
    }

    /// A branch Ref moves when the remote's commit is not the Lock's.
    fn by_remote(&mut self, repo: RepoRef) -> Result<Assessed, Stop> {
        let id = repo.id;
        let Some(locked) = self.lock.as_ref().and_then(|lock| lock.get(id)) else {
            return Ok(Assessed::Stay(format!(
                "`{id}` is not locked yet; `refs sync` locks it"
            )));
        };
        let now = match self.source.resolve(repo) {
            Err(SourceError::RefNotFound { .. }) => return self.ask_ref(repo),
            now => now.map_err(skip)?,
        };
        if now.sha() == locked.pin.sha() {
            return Ok(Assessed::Stay(format!("`{id}` is up to date")));
        }
        Ok(Assessed::Move(Move {
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
        }))
    }

    /// The Ref of `repo` is gone from the remote: ask for another, starting from the old one.
    fn ask_ref(&mut self, repo: RepoRef) -> Result<Assessed, Stop> {
        self.must_be_clean(repo.id)?;
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

    /// The reason not to move the Checkout of `id`, if there is one.
    fn hold_of(&self, id: &str) -> Option<String> {
        match self.source.inspect(id) {
            Ok(Observed::At { dirty_files, .. }) if !dirty_files.is_empty() => Some(format!(
                "its checkout has uncommitted changes ({}); `--force` discards them",
                dirty_files.join(", ")
            )),
            Err(e) => Some(format!("its checkout cannot be inspected: {e}")),
            _ => None,
        }
    }

    /// A Repo that moves is left alone if its Checkout has changes `--force` was not given for.
    fn must_be_clean(&self, id: &str) -> Result<(), Stop> {
        match self.force {
            true => Ok(()),
            false => self.hold_of(id).map_or(Ok(()), |why| Err(skip(why))),
        }
    }
}

fn moved_to(id: &str, from: &str, to: &str) -> Assessed {
    Assessed::Move(Move {
        id: id.into(),
        git_ref: Some(to.into()),
        look_at: to.into(),
        line: format!("{id}  {from} → {to}"),
        fields: Fields::default(),
    })
}
