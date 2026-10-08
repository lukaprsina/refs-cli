//! The real `Registry`: the three registries over HTTPS.
//!
//! It has no test, as tests never touch the network. After changing it, run `refs add npm:`,
//! `cargo:` and `pypi:` by hand against the real registries (`--no-sync` is enough).

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::Client;

use super::{Ecosystem, Failure, Found, Registry, RegistryError, crates, npm, pypi};

/// crates.io answers 403 to a request without a User-Agent that says who is asking.
const USER_AGENT: &str = concat!(
    "refs-cli/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/lukaprsina/refs-cli)"
);
const TIMEOUT: Duration = Duration::from_secs(15);

/// Asks npm, crates.io and PyPI. One lookup per `add`, far under crates.io's rate limit of a
/// request per second, so there is no retry.
pub struct Http;

impl Registry for Http {
    fn lookup(
        &self,
        ecosystem: Ecosystem,
        name: &str,
        version: Option<&str>,
    ) -> Result<Found, RegistryError> {
        let registry = ecosystem.registry();
        let request = |why: String| RegistryError::Request {
            registry,
            name: name.to_owned(),
            why,
        };
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| request(e.to_string()))?;
        let response = client
            .get(endpoint(ecosystem, name, version))
            .send()
            .map_err(|e| request(e.to_string()))?;
        match response.status() {
            StatusCode::NOT_FOUND => {
                return Err(RegistryError::NotFound {
                    registry,
                    name: name.to_owned(),
                });
            }
            status if !status.is_success() => {
                return Err(RegistryError::Status {
                    registry,
                    name: name.to_owned(),
                    status: status.as_u16(),
                });
            }
            _ => {}
        }
        let body = response.text().map_err(|e| request(e.to_string()))?;
        let found = match ecosystem {
            Ecosystem::Npm => npm::found(&body),
            Ecosystem::Cargo => crates::found(&body),
            Ecosystem::Pypi => pypi::found(&body),
        };
        found.map_err(|failure| match failure {
            Failure::NoRepository => RegistryError::NoRepository {
                registry,
                name: name.to_owned(),
            },
            Failure::Malformed(why) => RegistryError::Malformed {
                registry,
                name: name.to_owned(),
                why,
            },
        })
    }
}

/// The URL of the document that holds the repository: the cheapest one the registry serves,
/// or the one for `version` when it is given, since `repository` can differ between versions.
fn endpoint(ecosystem: Ecosystem, name: &str, version: Option<&str>) -> String {
    let name = encode(name);
    match (ecosystem, version) {
        (Ecosystem::Npm, version) => format!(
            "https://registry.npmjs.org/{name}/{}",
            version.map_or_else(|| "latest".to_owned(), encode)
        ),
        (Ecosystem::Cargo, None) => format!("https://crates.io/api/v1/crates/{name}?include="),
        (Ecosystem::Cargo, Some(version)) => {
            format!("https://crates.io/api/v1/crates/{name}/{}", encode(version))
        }
        (Ecosystem::Pypi, None) => format!("https://pypi.org/pypi/{name}/json"),
        (Ecosystem::Pypi, Some(version)) => {
            format!("https://pypi.org/pypi/{name}/{}/json", encode(version))
        }
    }
}

/// `text` as one URL path segment: everything but unreserved characters is percent-encoded,
/// so `@scope/name` is `%40scope%2Fname`.
fn encode(text: impl AsRef<str>) -> String {
    let mut encoded = String::new();
    for byte in text.as_ref().bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
