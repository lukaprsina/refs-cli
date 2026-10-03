//! Pure helpers for talking to a remote: what to ask, and how to read the answer.

use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::config::{is_full_sha, url_problem};
use crate::diagnostic::SourceError;

/// `<sha>\t<refname>` lines, as `git ls-remote` prints them.
fn lines(output: &str) -> impl Iterator<Item = (&str, &str)> {
    output.lines().filter_map(|l| l.split_once('\t'))
}

/// The one commit among `shas`, if they agree; `Err` when they name different ones.
fn single<'a>(mut shas: Vec<&'a str>, git_ref: &str) -> Result<Option<&'a str>, SourceError> {
    shas.dedup();
    match shas.as_slice() {
        [] => Ok(None),
        [sha] => Ok(Some(sha)),
        _ => Err(SourceError::AmbiguousRef {
            git_ref: git_ref.into(),
        }),
    }
}

/// The commit a branch or tag names, from `git ls-remote` output for the patterns
/// `refs/heads/<ref> refs/tags/<ref> refs/tags/<ref>^{}`. Those patterns tail-match, so
/// only the exact ref names count. A tag wins over a branch; an annotated tag's `^{}` line
/// is the commit, its plain line the tag object.
///
/// A tag with no `^{}` line is taken as lightweight: the output cannot tell it from an
/// annotated tag the server failed to peel. Rejecting a tag object (`refs::git::unpeeled_tag`)
/// needs the object type, so `verify` does it once the Cache has the object.
pub fn select_ref(url: &str, git_ref: &str, ls_remote: &str) -> Result<String, SourceError> {
    let named = |name: String| -> Vec<&str> {
        lines(ls_remote)
            .filter(|(_, n)| *n == name)
            .map(|(sha, _)| sha)
            .collect()
    };
    let tag = single(named(format!("refs/tags/{git_ref}")), git_ref)?;
    let peeled = single(named(format!("refs/tags/{git_ref}^{{}}")), git_ref)?;
    let branch = single(named(format!("refs/heads/{git_ref}")), git_ref)?;
    tag.map(|tag| peeled.unwrap_or(tag))
        .or(branch)
        .map(String::from)
        .ok_or_else(|| SourceError::RefNotFound {
            url: url.into(),
            git_ref: git_ref.into(),
            help: "a ref is a branch, a tag or a full 40-character commit id; abbreviated ids are not supported",
        })
}

/// The remote's default branch and its commit, from `git ls-remote --symref <url> HEAD`.
/// The branch is omitted when the remote's HEAD is detached (no `ref:` line).
pub fn select_head(url: &str, ls_remote: &str) -> Result<(String, Option<String>), SourceError> {
    let head = || lines(ls_remote).filter(|(_, name)| *name == "HEAD");
    let branch = head()
        .find_map(|(target, _)| target.strip_prefix("ref: refs/heads/"))
        .map(String::from);
    head()
        .map(|(sha, _)| sha)
        .find(|sha| is_full_sha(sha))
        .map(|sha| (sha.to_string(), branch))
        .ok_or_else(|| SourceError::RefNotFound {
            url: url.into(),
            git_ref: "HEAD".into(),
            help: "the remote has no commits to resolve HEAD to",
        })
}

const MINIMUM_VERSION: (u32, u32, u32) = (2, 36, 0);

/// `Ok` if `git --version` output names 2.36.0 or newer.
pub fn check_version(output: &str) -> Result<(), SourceError> {
    let found = output.trim();
    let mut parts = found
        .strip_prefix("git version ")
        .unwrap_or_default()
        .split(|c: char| !c.is_ascii_digit())
        .map(|p| p.parse::<u32>());
    let Some((Ok(major), Ok(minor), Ok(patch))) = parts
        .next()
        .zip(parts.next())
        .zip(parts.next())
        .map(|((a, b), c)| (a, b, c))
    else {
        return Err(SourceError::Failed {
            message: format!("could not read the git version from `{found}`"),
        });
    };
    if (major, minor, patch) < MINIMUM_VERSION {
        return Err(SourceError::TooOld {
            found: found.trim_start_matches("git version ").into(),
        });
    }
    Ok(())
}

/// Reject a `url` or `ref` git must never see: option-like, or a transport other than
/// https, ssh, git and file (`ext::`). `refs.toml` is checked on load too, but `GitSource`
/// does not rely on its caller.
pub fn check_input(url: &str, git_ref: &str) -> Result<(), SourceError> {
    if let Some((reason, _)) = url_problem(url) {
        return Err(SourceError::UnsafeInput {
            reason: reason.into(),
        });
    }
    if git_ref.starts_with('-') {
        return Err(SourceError::UnsafeInput {
            reason: "`ref` must not start with `-`: git would read it as an option".into(),
        });
    }
    Ok(())
}

/// Reject a commit id that is not 40 hex digits: it would reach git where an option could
/// be read. A Lock read from disk is checked already; `GitSource` does not rely on that.
pub fn check_sha(sha: &str) -> Result<(), SourceError> {
    if is_full_sha(sha) {
        return Ok(());
    }
    Err(SourceError::UnsafeInput {
        reason: format!("`{sha}` is not a full 40-character commit id"),
    })
}

/// The form of `url` the cache is keyed on: no trailing `/` or `.git`, lowercase host.
/// Protocols are not unified: that would mean guessing each host's URL mapping.
pub fn normalise_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let trimmed = trimmed.trim_end_matches('/');
    if let Some((scheme, rest)) = trimmed.split_once("://") {
        let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        return format!("{scheme}://{}{path}", lower_host(authority));
    }
    match trimmed.split_once(':') {
        Some((authority, path)) => format!("{}:{path}", lower_host(authority)),
        None => trimmed.to_string(),
    }
}

/// Lowercases what follows the last `@` (the host, and a port), keeping the user as written.
fn lower_host(authority: &str) -> String {
    match authority.rsplit_once('@') {
        Some((user, host)) => format!("{user}@{}", host.to_lowercase()),
        None => authority.to_lowercase(),
    }
}

/// The cache directory name for `url`: 16 hex characters of the SHA-256 of its normal form.
pub fn cache_dir_name(url: &str) -> String {
    Sha256::digest(normalise_url(url).as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What `git ls-tree <commit> -- <path>` found at `path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Missing,
    Tree,
    /// A file, a symlink or a submodule: anything that is not a directory.
    Other,
}

/// The kind of the first entry of `git ls-tree` output (`<mode> <type> <oid>\t<path>`).
pub fn entry_kind(ls_tree: &str) -> EntryKind {
    match ls_tree.split_whitespace().nth(1) {
        None => EntryKind::Missing,
        Some("tree") => EntryKind::Tree,
        Some(_) => EntryKind::Other,
    }
}

/// The distinct blob ids in `git ls-tree` output, in order. Submodule entries are skipped:
/// their commits are not in this repository.
pub fn tree_blobs(ls_tree: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    ls_tree
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(meta, _)| meta))
        .filter_map(|meta| {
            let mut fields = meta.split_whitespace().skip(1);
            (fields.next() == Some("blob")).then(|| fields.next())?
        })
        .filter(|oid| seen.insert(*oid))
        .map(String::from)
        .collect()
}

/// Whether a failed `git fetch <sha>` means the remote will not send that commit: it does not
/// have it, or refuses to serve a commit by its id. Git gives no other signal.
pub fn commit_unavailable(stderr: &str) -> bool {
    stderr.contains("not our ref") || stderr.contains("unadvertised object")
}

/// The object a checkout could not get, from `could not fetch <oid> from promisor remote`.
pub fn missing_object(stderr: &str) -> Option<String> {
    let rest = stderr.split_once("could not fetch ")?.1;
    let oid = rest.split_whitespace().next()?;
    (rest[oid.len()..]
        .trim_start()
        .starts_with("from promisor remote")
        && oid.len() >= 40
        && oid.bytes().all(|b| b.is_ascii_hexdigit()))
    .then(|| oid.to_string())
}

/// The directories above each of `paths`, as `ls-tree` wants them (`docs/`, `docs/guide/`),
/// each once, in order. Cone mode checks out the files directly inside them.
pub fn ancestor_dirs(paths: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    paths
        .iter()
        .flat_map(|path| {
            path.match_indices('/')
                .map(|(i, _)| format!("{}/", &path[..i]))
                .collect::<Vec<_>>()
        })
        .filter(|dir| seen.insert(dir.clone()))
        .collect()
}

/// The ids `git cat-file --batch-check` reported as missing.
pub fn missing_oids(batch_check: &str) -> Vec<String> {
    batch_check
        .lines()
        .filter_map(|line| line.strip_suffix(" missing"))
        .map(String::from)
        .collect()
}

/// The path in a Checkout's `.git` file (`gitdir: <path>`).
pub fn parse_gitdir(dot_git: &str) -> Option<&str> {
    let path = dot_git.lines().next()?.strip_prefix("gitdir:")?.trim();
    (!path.is_empty()).then_some(path)
}

/// `path` with `.` and `..` resolved by name alone, so a gitdir that no longer exists can
/// still be placed.
pub fn lexical_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The Cache entry a worktree's admin directory belongs to, if it is where git puts one:
/// `<cache_git_root>/<hash>/worktrees/<name>`.
pub fn worktree_entry(admin: &Path, cache_git_root: &Path) -> Option<String> {
    let rest = lexical_path(admin)
        .strip_prefix(lexical_path(cache_git_root))
        .ok()?
        .to_path_buf();
    let mut parts = rest.components().map(|c| c.as_os_str().to_str());
    let (entry, worktrees, name) = (parts.next()??, parts.next()??, parts.next()??);
    let is_entry = entry.len() == 16 && entry.bytes().all(|b| b.is_ascii_hexdigit());
    (is_entry && worktrees == "worktrees" && !name.is_empty() && parts.next().is_none())
        .then(|| entry.to_string())
}

/// The paths in `git status --porcelain=v1 -z` output (`XY <path>` entries, NUL separated).
pub fn dirty_files(porcelain_z: &str) -> Vec<String> {
    porcelain_z
        .split('\0')
        .filter_map(|entry| entry.get(3..))
        .filter(|path| !path.is_empty())
        .map(String::from)
        .collect()
}
