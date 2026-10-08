//! crates.io: `GET https://crates.io/api/v1/crates/{name}?include=` (or `/{name}/{version}`).
//! The fields are in the `crate` object, or in `version` for a version document.

use serde::Deserialize;

use super::url::{clean, forge};
use super::{Failure, Found};

#[derive(Deserialize)]
struct Document {
    #[serde(rename = "crate")]
    krate: Option<Entry>,
    version: Option<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    repository: Option<String>,
    homepage: Option<String>,
}

/// Where the crate in `body` lives: its `repository`, else its `homepage` when that is a
/// repository on a forge.
pub fn found(body: &str) -> Result<Found, Failure> {
    let document: Document =
        serde_json::from_str(body).map_err(|e| Failure::Malformed(e.to_string()))?;
    let entry = document.krate.or(document.version);
    let url = entry.and_then(|entry| {
        entry
            .repository
            .as_deref()
            .and_then(clean)
            .or_else(|| entry.homepage.as_deref().and_then(forge))
    });
    Ok(Found {
        url: url.ok_or(Failure::NoRepository)?,
        directory: None,
    })
}
