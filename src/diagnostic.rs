use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

use crate::tooling::{self, Gap};

/// What to do about a server that will not send an object by its id (the tail of two helps).
const FETCH_BY_ID: &str = "Fetching by id needs git protocol v2 (the default since git 2.26) or `uploadpack.allowReachableSHA1InWant` on the server; a proxy that drops the `Git-Protocol` header forces v0";

/// Every problem found in a `refs.toml`, reported together.
#[derive(Debug, Error, Diagnostic)]
#[error("invalid refs.toml")]
pub struct ConfigErrors {
    #[related]
    pub errors: Vec<ConfigError>,
}

/// Why an edit of `refs.toml` was not made. Nothing is written when one is returned.
#[derive(Debug, Error, Diagnostic)]
pub enum EditError {
    /// The config before the edit, or the text the edit would produce, fails validation.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Invalid(#[from] ConfigErrors),

    #[error("a repo with the id `{id}` already exists")]
    #[diagnostic(code(refs::config::id_taken), help("pick another id with `--id <id>`"))]
    IdTaken { id: String },

    #[error("there is no repo with the id `{id}`")]
    #[diagnostic(code(refs::config::unknown_repo), help("`refs list` shows the ids"))]
    UnknownRepo { id: String },

    #[error("there is no group with the id `{id}`")]
    #[diagnostic(
        code(refs::config::unknown_group),
        help("`refs list` shows the groups")
    )]
    UnknownGroup { id: String },

    #[error("could not edit refs.toml: {0}")]
    #[diagnostic(code(refs::config::unreadable))]
    Unreadable(String),
}

/// What is wrong with a `refs.toml`, apart from where.
#[derive(Debug, Error, Diagnostic)]
pub enum ConfigProblem {
    #[error("{message}")]
    #[diagnostic(code(refs::config::syntax))]
    Syntax { message: String },

    #[error("unknown key `{key}`")]
    #[diagnostic(code(refs::config::unknown_key), help("{expected}"))]
    UnknownKey { key: String, expected: String },

    #[error("`{name}` is not a tool refs knows")]
    #[diagnostic(
        code(refs::config::unknown_tool),
        help("use one of: {}", tooling::names().join(", "))
    )]
    UnknownTool { name: String },

    #[error("invalid repo id `{id}`")]
    #[diagnostic(
        code(refs::config::bad_id),
        help("use lowercase letters, digits, `.`, `_` and `-`, starting with a letter or digit")
    )]
    BadId { id: String },

    #[error("repo refers to unknown group `{group}`")]
    #[diagnostic(
        code(refs::config::dangling_group),
        help("define it under [groups.{group}]")
    )]
    DanglingGroup { group: String },

    #[error("`start` entry `{start}` is not in `paths`")]
    #[diagnostic(
        code(refs::config::start_outside_paths),
        help("use a file inside one of `paths`, or a file directly in the repo root")
    )]
    StartOutsidePaths { start: String },

    #[error("`{path}` must be a relative path without `..`")]
    #[diagnostic(code(refs::config::bad_path))]
    BadPath { path: String },

    #[error(
        "`{path}` must be a path inside the project: relative, without `..`, backticks or control characters"
    )]
    #[diagnostic(code(refs::config::bad_settings_path))]
    BadSettingsPath { path: String },

    #[error("{reason}")]
    #[diagnostic(code(refs::config::bad_url))]
    BadUrl {
        reason: &'static str,
        #[help]
        help: Option<&'static str>,
    },

    #[error("`ref` must not start with `-`")]
    #[diagnostic(code(refs::config::bad_ref))]
    BadRef,

    #[error("invalid `origin`: {why}")]
    #[diagnostic(code(refs::config::bad_origin))]
    BadOrigin { why: &'static str },

    #[error("invalid group name")]
    #[diagnostic(
        code(refs::config::bad_group_name),
        help(
            "use letters, digits, spaces and . , : ( ) / + & - only, with no leading or trailing space"
        )
    )]
    BadGroupName,

    #[error("text is rendered into the managed block and cannot contain this")]
    #[diagnostic(
        code(refs::config::unsafe_text),
        help("use a single line with no control characters, no ``` and no BEGIN:refs or END:refs")
    )]
    UnsafeText,
}

impl ConfigProblem {
    /// The text under the span in the rendered source.
    fn label(&self) -> &'static str {
        match self {
            ConfigProblem::Syntax { .. } => "here",
            ConfigProblem::UnknownKey { .. } => "not a refs.toml key",
            ConfigProblem::UnknownTool { .. } => "unknown tool",
            ConfigProblem::BadId { .. } => "not a valid directory name",
            ConfigProblem::DanglingGroup { .. } => "no such group",
            ConfigProblem::StartOutsidePaths { .. } => "not checked out",
            ConfigProblem::BadPath { .. } | ConfigProblem::BadSettingsPath { .. } => "not allowed",
            ConfigProblem::BadUrl { .. } => "rejected",
            ConfigProblem::BadRef => "git would read this as an option",
            ConfigProblem::BadOrigin { .. } => "rejected",
            ConfigProblem::BadGroupName => "rendered as a heading",
            ConfigProblem::UnsafeText => "rejected",
        }
    }
}

/// A `ConfigProblem` at a place in the `refs.toml` it was found in. Each error carries its own
/// copy of the source (ADR 0002).
#[derive(Debug, Error)]
#[error("{problem}")]
pub struct ConfigError {
    pub problem: ConfigProblem,
    src: NamedSource<String>,
    span: Option<SourceSpan>,
}

impl ConfigError {
    pub fn new(problem: ConfigProblem, src: NamedSource<String>, span: Option<SourceSpan>) -> Self {
        ConfigError { problem, src, span }
    }
}

impl Diagnostic for ConfigError {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        self.problem.code()
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        self.problem.help()
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.src)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        let label = self.problem.label();
        self.span.map(|span| {
            Box::new(std::iter::once(miette::LabeledSpan::new_with_span(
                Some(label.into()),
                span,
            ))) as _
        })
    }
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

    #[error("`start` file `{path}` in `{repo}` at {sha} is a directory that is not checked out")]
    #[diagnostic(
        code(refs::git::start_not_file),
        help(
            "with `paths` set, only files directly in the repo root are checked out besides them"
        )
    )]
    StartNotFile {
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
            "the commit may not exist, or the server refuses to send a commit by its id. {}",
            FETCH_BY_ID
        )
    )]
    CommitUnavailable { url: String, sha: String },

    #[error("the remote will not send some of the objects it was asked for")]
    #[diagnostic(
        code(refs::git::objects_unavailable),
        help(
            "the server may refuse to send an object by its id, or may not have it. {}",
            FETCH_BY_ID
        )
    )]
    ObjectsUnavailable,

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

impl SourceError {
    /// This failure as a report that names the Repo it happened to, unless the message
    /// already does. The code and help are the failure's own.
    pub fn for_repo(self, id: &str) -> miette::Report {
        match self {
            SourceError::PathMissing { .. }
            | SourceError::PathNotDir { .. }
            | SourceError::StartMissing { .. }
            | SourceError::StartNotFile { .. }
            | SourceError::ObjectMissing { .. } => miette::Report::new(self),
            cause => miette::Report::new(RepoFailed {
                id: id.into(),
                cause,
            }),
        }
    }
}

/// A `SourceError` with the id of the Repo it happened to.
#[derive(Debug, Error)]
#[error("repo `{id}`: {cause}")]
pub struct RepoFailed {
    pub id: String,
    #[source]
    pub cause: SourceError,
}

impl Diagnostic for RepoFailed {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        self.cause.code()
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        self.cause.help()
    }
}

/// A file that could not be read or written.
#[derive(Debug, Error, Diagnostic)]
pub enum FileError {
    #[error("could not read {path}")]
    #[diagnostic(code(refs::io::read_failed))]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write {path}")]
    #[diagnostic(code(refs::io::write_failed))]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

impl FileError {
    pub fn read(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        FileError::Read {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn write(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        FileError::Write {
            path: path.as_ref().display().to_string(),
            source,
        }
    }
}

/// A problem reading or writing `refs.lock`.
#[derive(Debug, Error, Diagnostic)]
pub enum LockError {
    #[error("invalid refs.lock: {message}")]
    #[diagnostic(
        code(refs::lock::invalid),
        help("refs.lock is machine-written; run `refs sync` to regenerate it")
    )]
    Invalid { message: String },

    #[error(transparent)]
    #[diagnostic(transparent)]
    File(#[from] FileError),
}

/// The markers of an Agent file are not exactly one BEGIN then one END, so `sync` refuses
/// rather than guess which text belongs to refs.
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
}

/// `plan_lock` was asked for something it must not do.
#[derive(Debug, Error, Diagnostic)]
pub enum LockRefusal {
    #[error("refs.lock is missing or out of date and --offline forbids resolving: {}", .drift.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    #[diagnostic(code(refs::lock::offline_stale), help("run `refs sync` while online"))]
    OfflineStale { drift: Vec<crate::plan::Drift> },
}

/// Why `plan_checkouts` will not touch something. `sync` refuses rather than delete what
/// refs did not create or what an agent edited; `--force` lifts only the second.
#[derive(Debug, Error, Diagnostic)]
pub enum Refusal {
    #[error("`{id}` is a directory refs did not create")]
    #[diagnostic(
        code(refs::sync::foreign_dir),
        help("move or delete it, then try again; refs never removes it, even with --force")
    )]
    ForeignDir { id: String },

    #[error("`{id}` has local changes: {}", .files.join(", "))]
    #[diagnostic(
        code(refs::sync::dirty_checkout),
        help("untracked files count too; pass `--force` to discard them")
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
#[derive(Debug, Clone, Error, Diagnostic)]
#[diagnostic(severity(Advice))]
pub enum Note {
    #[error("not a git repository, so the exclude rule for the references directory was not added")]
    #[diagnostic(code(refs::sync::no_git_repo))]
    NoGitRepo,

    #[error("`{id}` had a broken checkout; recreated it, local files in it were discarded")]
    #[diagnostic(code(refs::sync::recreated))]
    Recreated { id: String },

    #[error("{} does not exclude the references directory", .gap.tool)]
    #[diagnostic(code(refs::tooling::gap), help("{}", .gap.advice()))]
    ToolingGap { gap: Gap },

    #[error("editors may still list the references directory")]
    #[diagnostic(
        code(refs::tooling::editor_hint),
        help(
            "Zed (needed on Windows, not tried elsewhere): add `\"file_scan_exclusions\": [\"**/{references_dir}\", \"...\"]` to .zed/settings.json. \
             JetBrains (not tried): right-click {references_dir}, Mark Directory as, Excluded. \
             VS Code search, Helix and Neovim's ripgrep or fd pickers already skip it."
        )
    )]
    EditorHint { references_dir: String },
}

/// A problem with the Project directory or its output paths.
#[derive(Debug, Error, Diagnostic)]
pub enum ProjectError {
    #[error(transparent)]
    #[diagnostic(transparent)]
    File(#[from] FileError),

    #[error("no refs.toml in {start} or any parent directory")]
    #[diagnostic(code(refs::project::no_config), help("create one with `refs init`"))]
    NoConfig { start: String },

    #[error("{start} is inside the project at {root}")]
    #[diagnostic(
        code(refs::project::nested),
        help("`refs init --here` creates a nested project in this directory")
    )]
    Nested { start: String, root: String },

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
