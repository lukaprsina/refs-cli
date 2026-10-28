//! crates.io: `GET https://crates.io/api/v1/crates/{name}?include=`. The fields are
//! in the `crate` object.

use serde::Deserialize;

use super::url::{clean, on_forge};
use super::{Answer, Found, Spec, non_blank, parse};

pub(super) const SPEC: Spec = Spec {
    prefix: "cargo:",
    registry: "crates.io",
    url: "https://crates.io/api/v1/crates/{name}?include=",
    found,
    lockfiles: &["Cargo.lock"],
};

#[derive(Deserialize)]
struct Document {
    #[serde(rename = "crate")]
    krate: Option<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    description: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
}

/// Where the crate in `body` lives: its `repository`, else its `homepage` when that is a
/// repository on a forge.
pub fn found(body: &str) -> Answer {
    let document: Document = parse(body)?;
    let Some(entry) = document.krate else {
        return Ok(None);
    };
    let url = entry
        .repository
        .as_deref()
        .and_then(clean)
        .or_else(|| entry.homepage.as_deref().and_then(on_forge));
    Ok(url.map(|url| Found {
        url,
        directory: None,
        description: non_blank(entry.description),
    }))
}
