//! npm: `GET https://registry.npmjs.org/{name}/latest`, whose document
//! has `repository` as an object or a string.

use serde::Deserialize;

use super::url::{bare_github, clean};
use super::{Answer, Found, parse};

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
pub fn found(body: &str) -> Answer {
    let document: Document = parse(body)?;
    let (raw, directory) = match document.repository {
        Some(Repository::Text(raw)) => (raw, None),
        Some(Repository::Object {
            url: Some(raw),
            directory,
        }) => (raw, directory),
        _ => return Ok(None),
    };
    let url = if raw.contains(':') {
        clean(&raw)
    } else {
        bare_github(&raw)
    };
    Ok(url.map(|url| Found {
        url,
        directory: directory
            .map(|d| d.trim().trim_end_matches('/').to_owned())
            .filter(|d| !d.is_empty()),
    }))
}
