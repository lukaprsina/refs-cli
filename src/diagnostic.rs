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
