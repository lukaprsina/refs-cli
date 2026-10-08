// Copyright 2025 Vercel Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file
// except in compliance with the License. You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software distributed under the
// License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
// either express or implied. See the License for the specific language governing permissions
// and limitations under the License.
//
// Modified for refs-cli: see the module comment below.

//! Cleaning the repository URL a registry gives into one `add` can use.
//!
//! Derived from opensrc's `normalize_repo_url` and `extract_repo_url`
//! (`packages/opensrc/cli/src/core/registries/`, Apache-2.0, see NOTICE). Changed: the
//! spellings are handled by parsing the URL rather than by chained replaces, `scp`-style and
//! forge-shorthand URLs are understood, and a path is cut back to the repository on the hosts
//! where that is unambiguous.

/// Hosts whose repositories are always `owner/name`, so anything deeper is a page inside one.
const TWO_SEGMENT_HOSTS: [&str; 3] = ["github.com", "bitbucket.org", "codeberg.org"];

/// Path segments that start a page inside a repository on any host.
const PAGE_SEGMENTS: [&str; 10] = [
    "tree", "blob", "issues", "pulls", "pull", "wiki", "releases", "commit", "commits", "-",
];

const SHORTHANDS: [(&str, &str); 3] = [
    ("github:", "github.com"),
    ("gitlab:", "gitlab.com"),
    ("bitbucket:", "bitbucket.org"),
];

/// `raw` as an https git URL without `git+`, `.git`, a trailing slash, a fragment or a page
/// path; `None` when it does not name a repository, as when its scheme is neither http nor
/// https (`http` stays, for `config::url_problem` to reject like a hand-typed one).
pub(super) fn clean(raw: &str) -> Option<String> {
    cleaned(raw).map(|(_, url)| url)
}

/// `clean`, with the host the URL is on.
fn cleaned(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    for (prefix, host) in SHORTHANDS {
        if let Some(path) = raw.strip_prefix(prefix) {
            return cleaned(&format!("https://{host}/{path}"));
        }
    }
    let raw = raw.strip_prefix("git+").unwrap_or(raw);
    let (scheme, rest) = match raw.split_once("://") {
        Some((scheme, rest)) => (scheme, rest.to_owned()),
        None => {
            let scp = raw.strip_prefix("git@")?;
            ("ssh", scp.replacen(':', "/", 1))
        }
    };
    let (scheme, rest) = match scheme {
        "git" => ("https", rest.as_str()),
        "ssh" => ("https", rest.strip_prefix("git@").unwrap_or(&rest)),
        other => (other, rest.as_str()),
    };
    if !matches!(scheme, "https" | "http") {
        return None;
    }
    let rest = rest.split(['#', '?']).next()?;
    let mut segments = rest.split('/').filter(|s| !s.is_empty());
    let host = segments.next()?;
    let mut path: Vec<&str> = segments.collect();
    if let Some(page) = path.iter().skip(2).position(|s| PAGE_SEGMENTS.contains(s)) {
        path.truncate(page + 2);
    }
    if TWO_SEGMENT_HOSTS
        .iter()
        .any(|h| host.eq_ignore_ascii_case(h))
    {
        path.truncate(2);
    }
    let last = path.pop()?;
    path.push(last.strip_suffix(".git").unwrap_or(last));
    (path.len() >= 2).then(|| {
        (
            host.to_owned(),
            format!("{scheme}://{host}/{}", path.join("/")),
        )
    })
}

/// The URL for an npm `repository` string with no scheme and no prefix: `owner/name` is GitHub.
pub(super) fn bare_github(raw: &str) -> Option<String> {
    let (owner, name) = raw.trim().split_once('/')?;
    let word = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    (word(owner) && word(name)).then(|| format!("https://github.com/{owner}/{name}"))
}

/// Hosts that hold repositories, which makes their URLs trustworthy as a repository where a
/// registry field may as well hold a documentation site (crates.io `homepage`, PyPI `Homepage`).
const FORGES: [&str; 4] = ["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];

/// `clean`, but only for a repository on a forge. The host is compared whole, so
/// `github.com.attacker.example` and a `github.com` in the path do not pass.
pub(super) fn forge(raw: &str) -> Option<String> {
    let (host, url) = cleaned(raw)?;
    FORGES
        .iter()
        .any(|forge| host.eq_ignore_ascii_case(forge))
        .then_some(url)
}
