use indexmap::IndexMap;
use serde::Deserialize;
use toml::Spanned;

use miette::{NamedSource, SourceSpan};

use crate::diagnostic::{ConfigError, ConfigErrors};

pub type Id = Spanned<String>;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub groups: IndexMap<Id, Group>,
    #[serde(default)]
    pub repos: IndexMap<Id, Repo>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub references_dir: Option<String>,
    pub agents_files: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub name: Spanned<String>,
    pub description: Option<Spanned<String>>,
    pub enabled: Option<bool>,
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

impl Repo {
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
    for group in config.groups.values() {
        v.group(group);
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

    fn group(&mut self, group: &Group) {
        // the name becomes a heading; its charset check also covers marker text
        if !is_heading_safe(group.name.get_ref()) {
            self.report(&group.name, |src, span| ConfigError::BadGroupName {
                src,
                span,
            });
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

/// `url` reaches git from a file that may belong to an untrusted repository (spec §6.1).
fn url_problem(url: &str) -> Option<(&'static str, Option<&'static str>)> {
    if url.starts_with('-') {
        return Some((
            "`url` must not start with `-`: git would read it as an option",
            None,
        ));
    }
    // userinfo is what sits between `://` and the next `@` within the authority
    let authority = url.split_once("://")?.1.split(['/', '?', '#']).next()?;
    let (userinfo, _) = authority.rsplit_once('@')?;
    userinfo.contains(':').then_some((
        "`url` must not contain a password",
        Some("use a credential helper or an SSH agent instead"),
    ))
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
fn is_heading_safe(name: &str) -> bool {
    is_single_line_text(name)
        && !name.is_empty()
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && name.chars().all(|c| {
            c.is_alphanumeric()
                || matches!(c, ' ' | '.' | ',' | ':' | '(' | ')' | '/' | '+' | '&' | '-')
        })
}

fn is_relative_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && !path.split('/').any(|part| part == "..")
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
fn is_repo_id(id: &str) -> bool {
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
