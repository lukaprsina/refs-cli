//! crates.io: `GET https://crates.io/api/v1/crates/{name}?include=`. The fields are
//! in the `crate` object.

use serde::Deserialize;

use super::url::{clean, forge};
use super::{Answer, Found, parse};

#[derive(Deserialize)]
struct Document {
    #[serde(rename = "crate")]
    krate: Option<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    repository: Option<String>,
    homepage: Option<String>,
}

/// Where the crate in `body` lives: its `repository`, else its `homepage` when that is a
/// repository on a forge.
pub fn found(body: &str) -> Answer {
    let document: Document = parse(body)?;
    let url = document.krate.and_then(|entry| {
        entry
            .repository
            .as_deref()
            .and_then(clean)
            .or_else(|| entry.homepage.as_deref().and_then(forge))
    });
    Ok(url.map(|url| Found {
        url,
        directory: None,
    }))
}
