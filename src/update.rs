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
use crate::edit::{Edit, RefChange};
use crate::lock::Lock;
use crate::plan::Upgrade;
use crate::prompt::{Abort, Prompter};
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
            Ok(Assessed::Stay(note)) => say.status(note),
            Err(Halt::Skip(why)) => {
                say.shown(format!("`{id}` was not updated: {why}"));
                skipped += 1;
            }
            Err(Halt::Asked(abort)) => return Err(Failure::Asked(abort)),
        }
    }
    if moves.is_empty() {
        return Ok(Done {
            report: None,
            skipped,
        });
    }
    let changes: Vec<RefChange> = moves
        .iter()
        .filter_map(|found| match &found.how {
            How::Ref(to) => Some(RefChange {
                id: &found.id,
                git_ref: to,
            }),
            How::Commit => None,
        })
        .collect();
    let upgrade: Vec<String> = moves
        .iter()
        .filter(|found| matches!(found.how, How::Commit))
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
    let report = sync::edit(source, root, &Edit::Change(&changes), &flags);
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
    how: How,
    /// `id  old → new`
    line: String,
}

enum How {
    /// `refs.toml` gets this `ref`, and the Lock follows from the drift.
    Ref(String),
    /// Only the Lock moves, to the remote's commit now.
    Commit,
}

enum Assessed {
    Move(Move),
    /// Nothing to do, and the note says why.
    Stay(String),
}

/// Why a Repo is not assessed to a result.
enum Halt {
    /// Left as it is, for this reason.
    Skip(String),
    /// A question ended without an answer: the run ends.
    Asked(Abort),
}

impl From<Stop> for Halt {
    fn from(stop: Stop) -> Halt {
        match stop {
            Stop::Source(e) => Halt::Skip(e.to_string()),
            Stop::Asked(abort) => Halt::Asked(abort),
            Stop::Refused(why) => Halt::Skip(why),
        }
    }
}

fn skip(why: impl ToString) -> Halt {
    Halt::Skip(why.to_string())
}

struct Assessing<'a, 'p> {
    source: &'a dyn Source,
    root: &'a Path,
    lock: Option<Lock>,
    prompter: Option<&'a mut (dyn Prompter + 'p)>,
}

impl Assessing<'_, '_> {
    fn assess(&mut self, repo: RepoRef, request: &Request) -> Result<Assessed, Halt> {
        let found = match request.git_ref {
            Some(git_ref) => self.set(repo, git_ref)?,
            None => self.follow(repo)?,
        };
        if !request.force && matches!(found, Assessed::Move(_)) {
            self.check_clean(repo.id)?;
        }
        Ok(found)
    }

    /// `--ref`: the Ref asked for, if the remote has it.
    fn set(&self, repo: RepoRef, git_ref: &str) -> Result<Assessed, Halt> {
        let id = repo.id;
        let current = repo.repo.effective_ref();
        if git_ref == current {
            return Ok(Assessed::Stay(format!("`{id}` is already at `{current}`")));
        }
        let url = repo.repo.url.as_ref();
        self.source.outline(url, git_ref).map_err(skip)?;
        Ok(moved_to(id, current, git_ref))
    }

    /// Where the news about `repo` comes from depends on its Ref (see the module docs).
    fn follow(&mut self, repo: RepoRef) -> Result<Assessed, Halt> {
        let id = repo.id;
        let tags = self.source.tags(repo.repo.url.as_ref()).map_err(skip)?;
        let git_ref = repo.repo.git_ref.as_ref().map(|r| r.as_ref().as_str());
        match git_ref {
            Some(git_ref) if is_full_sha(git_ref) => Ok(Assessed::Stay(format!(
                "`{id}` is pinned to a commit; use `--ref` to move it"
            ))),
            Some(git_ref) if tags.iter().any(|tag| tag == git_ref) => {
                let Some(origin) = &repo.repo.origin else {
                    return Ok(Assessed::Stay(format!(
                        "`{id}` is pinned and has no origin; use `--ref` to move it"
                    )));
                };
                let package = Package::from_origin(origin.as_ref())
                    .expect("an origin is validated when the config loads");
                self.by_lockfile(repo, &package, git_ref, &tags)
            }
            _ => self.by_remote(repo),
        }
    }

    /// The Version tag of the version the project's Package lockfile has for `package`.
    fn by_lockfile(
        &mut self,
        repo: RepoRef,
        package: &Package,
        current: &str,
        tags: &[String],
    ) -> Result<Assessed, Halt> {
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
            Pick::Several(versions) => choose_version(
                self.prompter.as_deref_mut(),
                name,
                &versions,
                &file,
                &format!("use `refs update {id} --ref <tag>` to choose one"),
            )?,
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
    fn by_remote(&self, repo: RepoRef) -> Result<Assessed, Halt> {
        let id = repo.id;
        let Some(locked) = self.lock.as_ref().and_then(|lock| lock.get(id)) else {
            return Ok(Assessed::Stay(format!(
                "`{id}` is not locked yet; `refs sync` locks it"
            )));
        };
        let now = self.source.resolve(repo).map_err(skip)?;
        if now.sha() == locked.pin.sha() {
            return Ok(Assessed::Stay(format!("`{id}` is up to date")));
        }
        Ok(Assessed::Move(Move {
            id: id.into(),
            how: How::Commit,
            line: format!(
                "{id}  {} → {} ({})",
                locked.pin.short_id(),
                now.short_id(),
                repo.repo.effective_ref()
            ),
        }))
    }

    /// A Repo that moves is left alone if its Checkout has changes `--force` was not given for.
    fn check_clean(&self, id: &str) -> Result<(), Halt> {
        match self.source.inspect(id).map_err(skip)? {
            Observed::At { dirty_files, .. } if !dirty_files.is_empty() => Err(skip(format!(
                "its checkout has uncommitted changes ({}); `--force` discards them",
                dirty_files.join(", ")
            ))),
            _ => Ok(()),
        }
    }
}

fn moved_to(id: &str, from: &str, to: &str) -> Assessed {
    Assessed::Move(Move {
        id: id.into(),
        how: How::Ref(to.into()),
        line: format!("{id}  {from} → {to}"),
    })
}
