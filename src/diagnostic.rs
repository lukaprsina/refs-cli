use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

/// Every problem found in a `refs.toml`, reported together.
#[derive(Debug, Error, Diagnostic)]
#[error("invalid refs.toml")]
pub struct ConfigErrors {
    #[related]
    pub errors: Vec<ConfigError>,
}

#[derive(Debug, Error, Diagnostic)]
pub enum ConfigError {
    #[error("{message}")]
    #[diagnostic(code(refs::config::syntax))]
    Syntax {
        message: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("here")]
        span: Option<SourceSpan>,
    },

    #[error("unknown key `{key}`")]
    #[diagnostic(code(refs::config::unknown_key), help("{expected}"))]
    UnknownKey {
        key: String,
        expected: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("not a refs.toml key")]
        span: SourceSpan,
    },

    #[error("invalid repo id `{id}`")]
    #[diagnostic(
        code(refs::config::bad_id),
        help("use lowercase letters, digits, `.`, `_` and `-`, starting with a letter or digit")
    )]
    BadId {
        id: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("not a valid directory name")]
        span: SourceSpan,
    },

    #[error("repo refers to unknown group `{group}`")]
    #[diagnostic(
        code(refs::config::dangling_group),
        help("define it under [groups.{group}]")
    )]
    DanglingGroup {
        group: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("no such group")]
        span: SourceSpan,
    },

    #[error("`start` entry `{start}` is not in `paths`")]
    #[diagnostic(
        code(refs::config::start_outside_paths),
        help("use a file inside one of `paths`, or a file directly in the repo root")
    )]
    StartOutsidePaths {
        start: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("not checked out")]
        span: SourceSpan,
    },

    #[error("`{path}` must be a relative path without `..`")]
    #[diagnostic(code(refs::config::bad_path))]
    BadPath {
        path: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("not allowed")]
        span: SourceSpan,
    },

    #[error(
        "`{path}` must be a path inside the project: relative, without `..`, backticks or control characters"
    )]
    #[diagnostic(code(refs::config::bad_settings_path))]
    BadSettingsPath {
        path: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("not allowed")]
        span: SourceSpan,
    },

    #[error("{reason}")]
    #[diagnostic(code(refs::config::bad_url))]
    BadUrl {
        reason: &'static str,
        #[help]
        help: Option<&'static str>,
        #[source_code]
        src: NamedSource<String>,
        #[label("rejected")]
        span: SourceSpan,
    },

    #[error("`ref` must not start with `-`")]
    #[diagnostic(code(refs::config::bad_ref))]
    BadRef {
        #[source_code]
        src: NamedSource<String>,
        #[label("git would read this as an option")]
        span: SourceSpan,
    },

    #[error("invalid group name")]
    #[diagnostic(
        code(refs::config::bad_group_name),
        help(
            "use letters, digits, spaces and . , : ( ) / + & - only, with no leading or trailing space"
        )
    )]
    BadGroupName {
        #[source_code]
        src: NamedSource<String>,
        #[label("rendered as a heading")]
        span: SourceSpan,
    },

    #[error("text is rendered into the managed block and cannot contain this")]
    #[diagnostic(
        code(refs::config::unsafe_text),
        help("use a single line with no control characters, no ``` and no BEGIN:refs or END:refs")
    )]
    UnsafeText {
        #[source_code]
        src: NamedSource<String>,
        #[label("rejected")]
        span: SourceSpan,
    },
}

/// A failure reported by a `Source`.
#[derive(Debug, Clone, Error, Diagnostic)]
pub enum SourceError {
    #[error("{message}")]
    #[diagnostic(code(refs::git::failed))]
    Failed { message: String },

    #[error("`{git_ref}` was not found in {url}")]
    #[diagnostic(code(refs::git::ref_not_found), help("{help}"))]
    RefNotFound {
        url: String,
        git_ref: String,
        help: &'static str,
    },

    #[error("`{git_ref}` matches more than one ref in the remote")]
    #[diagnostic(code(refs::git::ambiguous_ref))]
    AmbiguousRef { git_ref: String },

    #[error("git {found} is too old; refs needs git 2.36.0 or newer")]
    #[diagnostic(
        code(refs::git::too_old),
        help("older versions mishandle sparse checkouts in worktrees of a bare repository")
    )]
    TooOld { found: String },

    #[error("{reason}")]
    #[diagnostic(code(refs::git::unsafe_input))]
    UnsafeInput { reason: String },

    #[error("`{path}` does not exist in `{repo}` at {sha}")]
    #[diagnostic(code(refs::git::path_missing))]
    PathMissing {
        repo: String,
        path: String,
        sha: String,
    },

    #[error("`{path}` in `{repo}` at {sha} is not a directory")]
    #[diagnostic(
        code(refs::git::path_not_dir),
        help("`paths` entries name directories; a file can be listed in `start`")
    )]
    PathNotDir {
        repo: String,
        path: String,
        sha: String,
    },

    #[error("`start` file `{path}` does not exist in `{repo}` at {sha}")]
    #[diagnostic(code(refs::git::start_missing))]
    StartMissing {
        repo: String,
        path: String,
        sha: String,
    },

    #[error("{sha} is a tag object, not a commit")]
    #[diagnostic(
        code(refs::git::unpeeled_tag),
        help(
            "the server did not peel this annotated tag; pin a commit id, or a ref the server can peel"
        )
    )]
    UnpeeledTag { sha: String },

    #[error("{url} does not serve commit {sha}")]
    #[diagnostic(
        code(refs::git::commit_unavailable),
        help(
            "the commit may not exist, or the server may refuse to send a commit by its id (see spec 7.4)"
        )
    )]
    CommitUnavailable { url: String, sha: String },

    #[error("{url} at {sha} is not in the cache, and `--offline` forbids fetching it")]
    #[diagnostic(
        code(refs::git::not_cached),
        help("run `refs sync` without `--offline` once to fill the cache")
    )]
    NotCached { url: String, sha: String },

    #[error("object {oid} of `{repo}` is not in the cache, and `--offline` forbids fetching it")]
    #[diagnostic(
        code(refs::git::object_missing),
        help("run `refs sync` without `--offline` once to fill the cache")
    )]
    ObjectMissing { repo: String, oid: String },
}

/// A problem reading or writing `refs.lock`.
#[derive(Debug, Error, Diagnostic)]
pub enum LockError {
    #[error("invalid refs.lock: {message}")]
    #[diagnostic(
        code(refs::lock::invalid),
        help("refs.lock is machine-written; run `refs lock` to regenerate it")
    )]
    Invalid { message: String },

    #[error("could not read {path}")]
    #[diagnostic(code(refs::lock::read_failed))]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write {path}")]
    #[diagnostic(code(refs::lock::write_failed))]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// A problem with an Agent file. The marker variants mean the markers are not exactly one
/// BEGIN then one END, so `sync` refuses rather than guess which text belongs to refs;
/// `Read` and `Write` are I/O failures.
#[derive(Debug, Error, Diagnostic)]
pub enum BlockError {
    #[error("a marker has no partner")]
    #[diagnostic(code(refs::block::unbalanced))]
    Unbalanced,

    #[error("a BEGIN marker comes before the previous one is closed")]
    #[diagnostic(code(refs::block::nested))]
    Nested,

    #[error("the END marker comes before the BEGIN marker")]
    #[diagnostic(code(refs::block::reversed))]
    Reversed,

    #[error("the markers appear more than once")]
    #[diagnostic(code(refs::block::duplicated))]
    Duplicated,

    #[error("could not read {path}")]
    #[diagnostic(code(refs::block::read_failed))]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write {path}")]
    #[diagnostic(code(refs::block::write_failed))]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// `render` was asked for Repos the Lock does not cover. `plan` makes sure the Lock is
/// current before it renders, so this is a caller bug, but writing a block that silently
/// leaves a Repo out would be worse.
#[derive(Debug, Error, Diagnostic)]
#[error("refs.lock has no entry for: {}", ids.join(", "))]
#[diagnostic(
    code(refs::render::not_locked),
    help("run `refs lock` to resolve them")
)]
pub struct NotLocked {
    pub ids: Vec<String>,
}

/// `plan_lock` was asked for something it must not do.
#[derive(Debug, Error, Diagnostic)]
pub enum LockRefusal {
    #[error("refs.lock is missing or out of date and --offline forbids resolving: {}", .drift.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    #[diagnostic(
        code(refs::lock::offline_stale),
        help("run `refs lock` while online, then sync again")
    )]
    OfflineStale { drift: Vec<crate::plan::Drift> },
    #[error("--upgrade resolves refs, which --offline forbids")]
    #[diagnostic(code(refs::lock::offline_upgrade))]
    OfflineUpgrade,
    #[error("--upgrade names repos that are not active: {}", .ids.join(", "))]
    #[diagnostic(code(refs::lock::unknown_id))]
    UnknownUpgradeId { ids: Vec<String> },
}

/// Why `plan_checkouts` will not touch something. `sync` refuses rather than delete what
/// refs did not create or what an agent edited; `--force` lifts only the second.
#[derive(Debug, Error, Diagnostic)]
pub enum Refusal {
    #[error("`{id}` is a directory refs did not create")]
    #[diagnostic(
        code(refs::sync::foreign_dir),
        help("move or delete it, then sync again; refs never removes it, even with --force")
    )]
    ForeignDir { id: String },

    #[error("`{id}` has local changes: {}", .files.join(", "))]
    #[diagnostic(
        code(refs::sync::dirty_checkout),
        help("untracked files count too; `refs sync --force` discards them")
    )]
    DirtyCheckout { id: String, files: Vec<String> },

    #[error("`{path}` has refs markers that are not exactly one BEGIN then one END")]
    #[diagnostic(
        code(refs::sync::bad_markers),
        help("fix or delete the markers by hand; refs will not guess which text is its own")
    )]
    Block {
        path: String,
        #[source]
        #[diagnostic_source]
        error: BlockError,
    },
}

/// An autofix `sync` announces; not drift.
#[derive(Debug, Error, Diagnostic)]
#[diagnostic(severity(Advice))]
pub enum Note {
    #[error("not a git repository, so the exclude rule for the references directory was not added")]
    #[diagnostic(code(refs::sync::no_git_repo))]
    NoGitRepo,

    #[error("`{id}` had a broken checkout; recreated it, local files in it were discarded")]
    #[diagnostic(code(refs::sync::recreated))]
    Recreated { id: String },
}

/// A problem reading or writing a project file other than `refs.lock` and the Agent files.
#[derive(Debug, Error, Diagnostic)]
pub enum ProjectError {
    #[error("could not read {path}")]
    #[diagnostic(code(refs::project::read_failed))]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write {path}")]
    #[diagnostic(code(refs::project::write_failed))]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("no refs.toml in {start} or any parent directory")]
    #[diagnostic(code(refs::project::no_config), help("create one with `refs init`"))]
    NoConfig { start: String },

    #[error("{path} resolves outside the project")]
    #[diagnostic(
        code(refs::project::escapes_root),
        help("an output path must stay inside the project, symlinks included")
    )]
    EscapesRoot { path: String },

    #[error("{path} is a symlink to nothing")]
    #[diagnostic(code(refs::project::broken_symlink))]
    BrokenSymlink { path: String },

    #[error("{path} exists and is not a directory")]
    #[diagnostic(code(refs::project::not_a_directory))]
    NotADirectory { path: String },

    #[error("{path} exists and is not a regular file")]
    #[diagnostic(code(refs::project::not_a_file))]
    NotAFile { path: String },
}
