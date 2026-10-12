//! The Forges refs knows, in one table: `add gh:owner/repo` and `gl:group/sub/repo` (a code
//! host's prefix and the path of the repository on it) and the cleaning of the repository URLs
//! registries give (`url`) both read it. The shorthand is input sugar like the registry
//! prefixes (ADR 0009), but pure, so it needs no network: `refs.toml` gets the https URL.

/// Where a host's repository path ends.
#[derive(Clone, Copy)]
pub(super) enum Depth {
    /// After `owner/repo`: anything deeper is a page inside it.
    Two,
    /// Before the first `-` segment (`/-/tree/main`). GitLab nests repositories in subgroups,
    /// so a segment such as `wiki` or `tree` may be a repository's name and only `/-/` is sure.
    UntilDash,
}

/// A host with the prefixes that name it.
pub(super) struct Forge {
    prefixes: &'static [&'static str],
    host: &'static str,
    pub(super) depth: Depth,
}

/// Every forge. Add a new one here only.
const ALL: [Forge; 4] = [
    Forge {
        prefixes: &["gh:", "github:"],
        host: "github.com",
        depth: Depth::Two,
    },
    Forge {
        prefixes: &["gl:", "gitlab:"],
        host: "gitlab.com",
        depth: Depth::UntilDash,
    },
    Forge {
        prefixes: &["bitbucket:"],
        host: "bitbucket.org",
        depth: Depth::Two,
    },
    Forge {
        prefixes: &[],
        host: "codeberg.org",
        depth: Depth::Two,
    },
];

/// The forge `host` is, compared whole and ignoring case, so `github.com.attacker.example` is
/// none.
pub(super) fn on(host: &str) -> Option<&'static Forge> {
    ALL.iter()
        .find(|forge| host.eq_ignore_ascii_case(forge.host))
}

/// `shorthand` as a forge's https URL without the host's repository rules applied, and the
/// forge: `bitbucket:o/r` is `https://bitbucket.org/o/r`. `None` without a forge prefix.
pub(super) fn expand_prefix(shorthand: &str) -> Option<(&'static Forge, String)> {
    let (forge, path) = split(shorthand)?;
    Some((forge, format!("https://{}/{path}", forge.host)))
}

/// The forge a shorthand's prefix names, and the path after the prefix.
fn split(shorthand: &str) -> Option<(&'static Forge, &str)> {
    ALL.iter().find_map(|forge| {
        let path = forge
            .prefixes
            .iter()
            .find_map(|prefix| shorthand.strip_prefix(prefix))?;
        Some((forge, path))
    })
}

/// The repository a forge shorthand names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The https URL of the repository.
    pub url: String,
    /// What followed the repository in the shorthand (`/tree/main/docs`), which is dropped.
    pub ignored: Option<String>,
}

/// What `shorthand` names, `None` when it has no forge prefix, or why it names no repository.
pub fn name(shorthand: &str) -> Option<Result<Named, &'static str>> {
    let (forge, path) = split(shorthand)?;
    Some(repository(forge, path))
}

fn repository(forge: &Forge, path: &str) -> Result<Named, &'static str> {
    if path.contains('@') {
        return Err("a ref is not part of it; pass `--ref` to pin one");
    }
    let (path, query) = path.split_at(path.find(['#', '?']).unwrap_or(path.len()));
    let segments: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    let keep = match forge.depth {
        Depth::Two => 2,
        Depth::UntilDash => segments
            .iter()
            .position(|s| *s == "-")
            .unwrap_or(segments.len()),
    }
    .min(segments.len());
    let (kept, rest) = segments.split_at(keep);
    if kept.len() < 2 {
        return Err("it needs an owner and a repository, as in `owner/repo`");
    }
    let (last, owners) = kept.split_last().unwrap_or((&"", &[]));
    let last = last.strip_suffix(".git").unwrap_or(last);
    if !owners.iter().copied().chain([last]).all(segment) {
        return Err("owner and repository use letters, digits, `.`, `_` and `-` only");
    }
    let ignored = if rest.is_empty() {
        query.to_owned()
    } else {
        format!("/{}{query}", rest.join("/"))
    };
    Ok(Named {
        url: format!("https://{}/{}/{last}", forge.host, owners.join("/")),
        ignored: Some(ignored).filter(|ignored| !ignored.is_empty()),
    })
}

/// A path segment that is a name: not empty, not `.` or `..`, not an option.
fn segment(segment: &str) -> bool {
    !segment.starts_with('-')
        && !segment.chars().all(|c| c == '.')
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}
