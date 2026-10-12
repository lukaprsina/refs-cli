//! crates.io: `GET https://crates.io/api/v1/crates/{name}?include=`. The fields are
//! in the `crate` object.

use serde::Deserialize;

use super::url::{clean, on_forge};
use super::used::cargo;
use super::{Answer, Found, Spec, parse};

pub(super) const SPEC: Spec = Spec {
    prefix: "cargo:",
    registry: "crates.io",
    url: "https://crates.io/api/v1/crates/{name}?include=",
    found,
    lockfiles: &[("Cargo.lock", cargo::used)],
};

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
            .or_else(|| entry.homepage.as_deref().and_then(on_forge))
    });
    Ok(url.map(|url| Found {
        url,
        directory: None,
    }))
}
