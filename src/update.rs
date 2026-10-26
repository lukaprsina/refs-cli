//! `refs update <id…>` (ADR 0012): move the named Repos to where their Ref should be now, in one
//! edit and one sync. What "now" means depends on the Ref. A tag Ref of a Repo with an Origin
//! follows the version the project's Package lockfile says it uses, through its Version tag; when
//! no lockfile answers (no Origin, no lockfile, or the Package is not in it) it moves to the
//! newest tag of its own kind (`tag::newest`). A branch Ref (no Ref, or one that is not a tag)
//! follows the remote, and moves when the remote's commit differs from the Lock's; a commit has
//! no news and a note pointing at `--ref`. `--ref` sets the Ref by hand. The Registry is never asked.
//! `--list` stops after that assessment and prints what each Repo would do and break; so does a
//! bare run with no one to ask, which says so.
//!
//! Everything that can be found out before the edit is: a Repo that cannot be assessed, whose
//! new Ref does not exist or whose Checkout has uncommitted changes (unless forced) is skipped
//! and reported, and the rest go through. What the remote does after the edit (stage 1 resolves
//! and verifies the whole edited config) blocks the run, as it does for any edit.

use std::path::Path;

mod ask;
mod assess;

use crate::active::active;
use crate::add::pin::Stop;
use crate::config::Config;
use crate::edit::{Edit, RepoChange};
use crate::failure::Failure;
use crate::lock::Lock;
use crate::source::Source;
use crate::sync::{self, Report, SyncFlags};
use crate::ui::{Prompter, Say};
use assess::{Assessing, Move};

/// What `refs update` was asked for.
pub struct Request<'a> {
    pub ids: &'a [String],
    /// `--ref`: the Ref to set, for the one Repo named.
    pub git_ref: Option<&'a str>,
    /// Move a Repo whose Checkout has uncommitted changes, discarding them.
    pub force: bool,
    /// `--list`: say what would move and what it would break; ask nothing, change nothing. A
    /// bare run with no one to ask comes to the same (see `Mode`).
    pub list: bool,
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

/// What a run does, from the request and whether there is someone to ask.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Move the Repos named.
    Named,
    /// Offer every Repo and move the ones picked.
    Pick,
    /// Say what each Repo would do and break; ask nothing, change nothing. Also what a bare
    /// `refs update` comes to with no one to ask.
    List,
}

impl Mode {
    fn of(request: &Request, asking: bool) -> Mode {
        match (request.list, request.ids.is_empty(), asking) {
            (true, ..) | (false, true, false) => Mode::List,
            (false, false, _) => Mode::Named,
            (false, true, true) => Mode::Pick,
        }
    }
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
    let mode = Mode::of(request, prompter.is_some());
    let ids: Vec<&str> = match request.ids {
        [] => active.repos().map(|repo| repo.id).collect(),
        named => named.iter().map(String::as_str).collect(),
    };
    let lock = Lock::read(&Lock::path(root)).map_err(Failure::Lock)?;
    let mut assessing = Assessing::new(source, root, lock, prompter, &mut *say, request.force);
    let mut seen = Vec::<&str>::new();
    let mut found = Vec::new();
    if mode != Mode::Named && !ids.is_empty() {
        if !request.list && request.ids.is_empty() && mode == Mode::List {
            assessing
                .say
                .status("no terminal: listing only; name repos to update".into());
        }
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
    if mode == Mode::List {
        return Ok(Done {
            report: None,
            skipped: 0,
            listing: assessing.listing(&found),
        });
    }
    let chosen = match mode {
        Mode::Pick => assessing.pick(&found).map_err(Failure::Asked)?,
        _ => seen.clone(),
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
    let reresolve: Vec<String> = moves
        .iter()
        .filter(|found| found.git_ref.is_none())
        .map(|found| found.id.clone())
        .collect();
    let flags = SyncFlags {
        force,
        reresolve,
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
