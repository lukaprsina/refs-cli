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
/// `Write` is an I/O failure.
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
