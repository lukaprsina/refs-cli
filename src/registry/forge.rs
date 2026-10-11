//! `add gh:owner/repo` and `gl:group/sub/repo`: a code host's prefix and the path of the
//! repository on it. Input sugar like the registry prefixes (ADR 0009), but pure, so it needs no
//! network: `refs.toml` gets the https URL.

/// Where a host's repository path ends.
#[derive(Clone, Copy)]
enum Depth {
    /// After `owner/repo`: anything deeper is a page inside it.
    Two,
    /// Before the first `-` segment (`/-/tree/main`). GitLab nests repositories in subgroups,
    /// so a segment such as `wiki` or `tree` may be a repository's name and only `/-/` is sure.
    UntilDash,
}

/// A host with the prefixes that name it.
struct Forge {
    prefixes: [&'static str; 2],
    host: &'static str,
    depth: Depth,
}

/// Every forge, for finding the one a shorthand names. Add a new one here too.
const ALL: [Forge; 2] = [
    Forge {
        prefixes: ["gh:", "github:"],
        host: "github.com",
        depth: Depth::Two,
    },
    Forge {
        prefixes: ["gl:", "gitlab:"],
        host: "gitlab.com",
        depth: Depth::UntilDash,
    },
];

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
    let (forge, path) = ALL.iter().find_map(|forge| {
        let path = forge
            .prefixes
            .iter()
            .find_map(|prefix| shorthand.strip_prefix(prefix))?;
        Some((forge, path))
    })?;
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
