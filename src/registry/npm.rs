//! npm: `GET https://registry.npmjs.org/{name}/latest` (or `/{name}/{version}`), whose
//! document has `repository` as an object or a string.

use serde::Deserialize;

use super::url::{bare_github, clean};
use super::{Failure, Found};

#[derive(Deserialize)]
struct Document {
    repository: Option<Repository>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Repository {
    Text(String),
    Object {
        url: Option<String>,
        directory: Option<String>,
    },
}

/// Where the package in `body` lives.
pub fn found(body: &str) -> Result<Found, Failure> {
    let document: Document =
        serde_json::from_str(body).map_err(|e| Failure::Malformed(e.to_string()))?;
    let (raw, directory) = match document.repository {
        Some(Repository::Text(raw)) => (raw, None),
        Some(Repository::Object {
            url: Some(raw),
            directory,
        }) => (raw, directory),
        _ => return Err(Failure::NoRepository),
    };
    let url = if raw.contains(':') {
        clean(&raw)
    } else {
        bare_github(&raw)
    };
    Ok(Found {
        url: url.ok_or(Failure::NoRepository)?,
        directory: directory
            .map(|d| d.trim().trim_end_matches('/').to_owned())
            .filter(|d| !d.is_empty()),
    })
}
