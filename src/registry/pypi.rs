//! PyPI: `GET https://pypi.org/pypi/{name}/json` (or `/{name}/{version}/json`). The repository
//! is one of `info.project_urls`, whose labels are free text and vary in case.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::url::{clean, forge};
use super::{Failure, Found};

#[derive(Deserialize)]
struct Document {
    info: Info,
}

#[derive(Deserialize)]
struct Info {
    project_urls: Option<BTreeMap<String, String>>,
    home_page: Option<String>,
}

/// The labels that name a repository, best first. Any host is trusted for these.
const SOURCE_LABELS: [&str; 6] = [
    "source",
    "source code",
    "repository",
    "code",
    "github",
    "gitlab",
];

/// Where the project in `body` lives: a source label, else a homepage that is a repository on a
/// forge. Other links (funding, documentation) are not guessed at.
pub fn found(body: &str) -> Result<Found, Failure> {
    let document: Document =
        serde_json::from_str(body).map_err(|e| Failure::Malformed(e.to_string()))?;
    let urls = document.info.project_urls.unwrap_or_default();
    let by_label = |wanted: &str| {
        urls.iter()
            .find(|(label, _)| label.trim().eq_ignore_ascii_case(wanted))
            .map(|(_, url)| url.as_str())
    };
    let url = SOURCE_LABELS
        .iter()
        .find_map(|label| by_label(label).and_then(clean))
        .or_else(|| by_label("homepage").and_then(forge))
        .or_else(|| document.info.home_page.as_deref().and_then(forge));
    Ok(Found {
        url: url.ok_or(Failure::NoRepository)?,
        directory: None,
    })
}
