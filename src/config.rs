use indexmap::IndexMap;
use serde::Deserialize;
use toml::Spanned;

use miette::{NamedSource, SourceSpan};

use crate::diagnostic::{ConfigError, ConfigErrors};
use crate::tooling::Tool;

pub type Id = Spanned<String>;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Tools to leave out of the tooling gap notes, by name (`Tool::names`).
    #[serde(default)]
    pub tooling_ignore: Vec<Spanned<String>>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub groups: IndexMap<Id, Group>,
    #[serde(default)]
    pub repos: IndexMap<Id, Repo>,
}

impl Config {
    /// The tools named in `tooling_ignore`. Validation has refused any other name.
    pub fn tooling_ignored(&self) -> Vec<Tool> {
        self.tooling_ignore
            .iter()
            .filter_map(|name| Tool::from_name(name.get_ref()))
            .collect()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub references_dir: Option<Spanned<String>>,
    pub agents_files: Option<Vec<Spanned<String>>>,
}

const DEFAULT_REFERENCES_DIR: &str = ".references";
const DEFAULT_AGENTS_FILE: &str = "AGENTS.md";

impl Settings {
    /// The Agent files, project-relative: `AGENTS.md` when none are configured.
    pub fn agents_files(&self) -> Vec<String> {
        match &self.agents_files {
            Some(files) => files.iter().map(|f| f.get_ref().clone()).collect(),
            None => vec![DEFAULT_AGENTS_FILE.into()],
        }
    }

    /// The references directory, project-relative, without a trailing `/`: `.references`
    /// when none is configured. This is what `render` pastes into the block's prose.
    pub fn references_dir(&self) -> &str {
        self.references_dir
            .as_ref()
            .map_or(DEFAULT_REFERENCES_DIR, |dir| {
                dir.get_ref().trim_end_matches('/')
            })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// The heading in the block; the group's id when absent (`add --group` makes bare groups).
    pub name: Option<Spanned<String>>,
    pub description: Option<Spanned<String>>,
    pub enabled: Option<bool>,
}

impl Group {
    /// The heading: the `name`, or the group's `id` when it has none.
    pub fn title<'a>(&'a self, id: &'a str) -> &'a str {
        self.name.as_ref().map_or(id, |name| name.get_ref())
    }

    /// The heading in `refs list`: `name (id)`, or just the name when it is the id.
    pub fn listed_as(&self, id: &str) -> String {
        match self.title(id) {
            title if title == id => title.to_string(),
            title => format!("{title} ({id})"),
        }
    }
}

/// A Repo with its id. The `Repo` carries no id (the map is keyed by a spanned id), so
/// this pair is the unit everything downstream takes.
#[derive(Debug, Clone, Copy)]
pub struct RepoRef<'a> {
    pub id: &'a str,
    pub repo: &'a Repo,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repo {
    pub url: Spanned<String>,
    pub group: Option<Spanned<String>>,
    #[serde(rename = "ref")]
    pub git_ref: Option<Spanned<String>>,
    pub description: Option<Spanned<String>>,
    #[serde(default)]
    pub paths: Vec<Spanned<String>>,
    #[serde(default)]
    pub packages: Vec<Spanned<String>>,
    #[serde(default)]
    pub start: Vec<Spanned<String>>,
    pub enabled: Option<bool>,
}

/// A full commit id: 40 lowercase hex characters.
pub(crate) fn is_full_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Repo {
    /// Whether the ref can move upstream: anything but a full 40-hex commit id.
    pub fn is_floating(&self) -> bool {
        !is_full_sha(self.effective_ref())
    }

    /// `paths` as plain strings, as the Lock and `Observed` hold them.
    pub fn path_strings(&self) -> Vec<String> {
        self.paths.iter().map(|p| p.get_ref().clone()).collect()
    }

    /// The ref to resolve: `HEAD` when none is configured.
    pub fn effective_ref(&self) -> &str {
        self.git_ref
            .as_ref()
            .map_or("HEAD", |r| r.as_ref().as_str())
    }
}

pub fn parse(text: &str) -> Result<Config, ConfigErrors> {
    let config: Config = toml::from_str(text).map_err(|e| ConfigErrors {
        errors: vec![from_toml_error(&e, text)],
    })?;
    let errors = validate(&config, text);
    if errors.is_empty() {
        Ok(config)
    } else {
        Err(ConfigErrors { errors })
    }
}

/// Semantic checks. Unlike parsing these all run, so every problem is reported at once.
fn validate(config: &Config, text: &str) -> Vec<ConfigError> {
    let mut v = Validator {
        text,
        errors: Vec::new(),
    };
    v.settings(&config.settings);
    v.tooling_ignore(&config.tooling_ignore);
    for (id, group) in &config.groups {
        v.group(id, group);
    }
    for (id, repo) in &config.repos {
        v.repo(config, id, repo);
    }
    v.errors
}

struct Validator<'a> {
    text: &'a str,
    errors: Vec<ConfigError>,
}

impl Validator<'_> {
    /// Build an error that points at `at`, carrying its own copy of the source (ADR 0002).
    fn report<T>(
        &mut self,
        at: &Spanned<T>,
        make: impl FnOnce(NamedSource<String>, SourceSpan) -> ConfigError,
    ) {
        let r = at.span();
        self.errors
            .push(make(source(self.text), (r.start, r.end - r.start).into()));
    }

    fn settings(&mut self, settings: &Settings) {
        // `references_dir` is pasted into the block's prose between backticks
        let dir = settings.references_dir.iter();
        let files = settings.agents_files.iter().flatten();
        for path in dir.chain(files) {
            if !is_project_path(path.get_ref()) {
                let bad = path.get_ref().clone();
                self.report(path, |src, span| ConfigError::BadSettingsPath {
                    path: bad,
                    src,
                    span,
                });
            }
        }
    }

    fn tooling_ignore(&mut self, names: &[Spanned<String>]) {
        for name in names {
            if Tool::from_name(name.get_ref()).is_none() {
                let unknown = name.get_ref().clone();
                self.report(name, |src, span| ConfigError::UnknownTool {
                    name: unknown,
                    src,
                    span,
                });
            }
        }
    }

    fn group(&mut self, id: &Id, group: &Group) {
        // the name (or, without one, the id) becomes a heading; its charset check also
        // covers marker text
        let heading = group.name.as_ref().unwrap_or(id);
        if !is_heading_safe(heading.get_ref()) {
            self.report(heading, |src, span| ConfigError::BadGroupName { src, span });
        }
        for text in group.description.iter() {
            self.text_line(text);
        }
    }

    fn repo(&mut self, config: &Config, id: &Id, repo: &Repo) {
        if !is_repo_id(id.get_ref()) {
            let id_text = id.get_ref().clone();
            self.report(id, |src, span| ConfigError::BadId {
                id: id_text,
                src,
                span,
            });
        }
        if let Some(group) = &repo.group
            && !config.groups.contains_key(group.get_ref().as_str())
        {
            let name = group.get_ref().clone();
            self.report(group, |src, span| ConfigError::DanglingGroup {
                group: name,
                src,
                span,
            });
        }
        if let Some((reason, help)) = url_problem(repo.url.get_ref()) {
            self.report(&repo.url, |src, span| ConfigError::BadUrl {
                reason,
                help,
                src,
                span,
            });
        }
        if let Some(git_ref) = &repo.git_ref
            && git_ref.get_ref().starts_with('-')
        {
            self.report(git_ref, |src, span| ConfigError::BadRef { src, span });
        }
        for text in repo
            .description
            .iter()
            .chain(&repo.packages)
            .chain(&repo.start)
        {
            self.text_line(text);
        }
        for path in repo.paths.iter().chain(&repo.start) {
            if !is_relative_path(path.get_ref()) {
                let bad = path.get_ref().clone();
                self.report(path, |src, span| ConfigError::BadPath {
                    path: bad,
                    src,
                    span,
                });
            }
        }
        // a start that is not a relative path was already reported as `bad_path`
        if !repo.paths.is_empty() {
            for start in &repo.start {
                if is_relative_path(start.get_ref())
                    && !start_is_checked_out(start.get_ref(), &repo.paths)
                {
                    let bad = start.get_ref().clone();
                    self.report(start, |src, span| ConfigError::StartOutsidePaths {
                        start: bad,
                        src,
                        span,
                    });
                }
            }
        }
    }

    fn text_line(&mut self, text: &Spanned<String>) {
        if !is_single_line_text(text.get_ref()) {
            self.report(text, |src, span| ConfigError::UnsafeText { src, span });
        }
    }
}

/// A reason and an optional help line, for `ConfigError::BadUrl`.
pub(crate) type UrlProblem = (&'static str, Option<&'static str>);

const NO_PASSWORD: UrlProblem = (
    "`url` must not contain a password",
    Some("use a credential helper or an SSH agent instead"),
);

const UNSUPPORTED: UrlProblem = (
    "`url` must be https, ssh, git or file, or scp-style `user@host:path`",
    Some("`http://`, `ext::` and other transports are not allowed"),
);

/// `url` reaches git from a file that may belong to an untrusted repository (spec §6.1).
/// Allowed: `https://`, `ssh://`, `git://`, `file://` and scp-style `user@host:path`, with
/// no password. `GIT_ALLOW_PROTOCOL` (spec §7.7) is the second layer.
pub(crate) fn url_problem(url: &str) -> Option<UrlProblem> {
    if url.starts_with('-') {
        return Some((
            "`url` must not start with `-`: git would read it as an option",
            None,
        ));
    }
    // What follows the first `:` tells the forms apart: `//` is a scheme URL, `:` is git's
    // `transport::address` syntax (`ext::`, `fd::`), anything else can only be scp-style.
    let after_colon = url.split_once(':').map(|(_, rest)| rest);
    if after_colon.is_some_and(|rest| rest.starts_with(':')) {
        return Some(UNSUPPORTED);
    }
    if let Some(rest) = after_colon.and_then(|rest| rest.strip_prefix("//")) {
        let scheme = url.split_once(':').map_or("", |(scheme, _)| scheme);
        if !matches!(scheme, "https" | "ssh" | "git" | "file") {
            return Some(UNSUPPORTED);
        }
        // userinfo is what sits between `://` and the next `@` within the authority
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        return has_password(authority).then_some(NO_PASSWORD);
    }
    // scp-style: a user, then `@`, a host, `:` and a path. A `/` before the first `:` would
    // make git read a local path, so neither the user nor the host may contain one, and a
    // host starting with `-` would be read as an option.
    let Some((userinfo, rest)) = url.split_once('@') else {
        return Some(UNSUPPORTED);
    };
    let Some((host, _)) = rest.split_once(':') else {
        return Some(UNSUPPORTED);
    };
    if userinfo.is_empty()
        || userinfo.contains('/')
        || host.is_empty()
        || host.contains('/')
        || host.starts_with('-')
    {
        return Some(UNSUPPORTED);
    }
    has_password(&format!("{userinfo}@")).then_some(NO_PASSWORD)
}

/// Whether the part before the last `@` of `authority` is `user:password`.
fn has_password(authority: &str) -> bool {
    authority
        .rsplit_once('@')
        .is_some_and(|(userinfo, _)| userinfo.contains(':'))
}

/// Free text lands inside the block's `text` fence, so it must stay on one line and
/// must not contain the fence or either marker (spec §6.1).
fn is_single_line_text(text: &str) -> bool {
    !text.chars().any(char::is_control)
        && !text.contains("```")
        && !text.contains("BEGIN:refs")
        && !text.contains("END:refs")
}

/// A group name becomes a `###` heading: letters, digits, spaces and `. , : ( ) / + & -`,
/// not starting or ending with a space, and no marker text (`:` is allowed, so `BEGIN:refs` would pass).
pub(crate) fn is_heading_safe(name: &str) -> bool {
    is_single_line_text(name)
        && !name.is_empty()
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && name.chars().all(|c| {
            c.is_alphanumeric()
                || matches!(c, ' ' | '.' | ',' | ':' | '(' | ')' | '/' | '+' | '&' | '-')
        })
}

/// `/`-separated, so a config means the same on every platform: a `\` or a `:` (a Windows
/// separator or drive) would make `..\x` or `C:\x` escape the project there.
fn is_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':'])
        && !path.split('/').any(|part| part == "..")
}

/// A `settings` output path: relative, inside the project, not the project root itself, and
/// safe to paste into the block's prose (no backtick, no control character, no marker).
fn is_project_path(path: &str) -> bool {
    is_relative_path(path)
        && path.split('/').any(|part| !matches!(part, "" | "."))
        && is_single_line_text(path)
        && !path.contains('`')
}

/// Cone mode checks out every `paths` entry plus the files directly in the repo root.
fn start_is_checked_out(start: &str, paths: &[Spanned<String>]) -> bool {
    !start.contains('/')
        || paths.iter().any(|p| {
            let dir = p.as_ref().trim_end_matches('/');
            start
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/'))
        })
}

/// `[a-z0-9][a-z0-9._-]*`
pub(crate) fn is_repo_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

fn source(text: &str) -> NamedSource<String> {
    NamedSource::new("refs.toml", text.to_string())
}

/// serde reports an unknown key as a message beginning "unknown field `x`, expected ...";
/// `toml` offers no structured variant for it.
fn from_toml_error(e: &toml::de::Error, text: &str) -> ConfigError {
    let span = e.span().map(|r| (r.start, r.end - r.start).into());
    let message = e.message();
    if let (Some(span), Some(rest)) = (span, message.strip_prefix("unknown field `"))
        && let Some((key, expected)) = rest.split_once("`, ")
    {
        return ConfigError::UnknownKey {
            key: key.to_string(),
            expected: expected.to_string(),
            src: source(text),
            span,
        };
    }
    ConfigError::Syntax {
        message: message.to_string(),
        src: source(text),
        span,
    }
}
