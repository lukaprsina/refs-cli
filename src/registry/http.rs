//! The real `Registry`: the three registries over HTTPS.
//!
//! It has no test, as tests never touch the network. After changing it, run `refs add npm:`,
//! `cargo:` and `pypi:` by hand against the real registries (`--no-sync` is enough).

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::blocking::Client;

use super::{Answer, Ecosystem, Found, Registry, RegistryError, crates, npm, pypi};

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
    fn lookup(&self, ecosystem: Ecosystem, name: &str) -> Result<Found, RegistryError> {
        let name_owned = || name.to_owned();
        let request = |why: String| RegistryError::Request {
            ecosystem,
            name: name_owned(),
            why,
        };
        let (url, read) = endpoint(ecosystem, name);
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| request(e.to_string()))?;
        let response = client.get(url).send().map_err(|e| request(e.to_string()))?;
        match response.status() {
            StatusCode::NOT_FOUND => {
                return Err(RegistryError::NotFound {
                    ecosystem,
                    name: name_owned(),
                });
            }
            status if !status.is_success() => {
                return Err(RegistryError::Status {
                    ecosystem,
                    name: name_owned(),
                    status: status.as_u16(),
                });
            }
            _ => {}
        }
        let body = response.text().map_err(|e| request(e.to_string()))?;
        match read(&body) {
            Ok(Some(found)) => Ok(found),
            Ok(None) => Err(RegistryError::NoRepository {
                ecosystem,
                name: name_owned(),
            }),
            Err(why) => Err(RegistryError::Malformed {
                ecosystem,
                name: name_owned(),
                why,
            }),
        }
    }
}

/// An adapter: reads a registry's document for where the package lives.
type Adapter = fn(&str) -> Answer;

/// The URL of the document that holds the repository, the cheapest one the registry serves,
/// and the adapter that reads it.
fn endpoint(ecosystem: Ecosystem, name: &str) -> (String, Adapter) {
    let name = encode(name);
    match ecosystem {
        Ecosystem::Npm => (
            format!("https://registry.npmjs.org/{name}/latest"),
            npm::found,
        ),
        Ecosystem::Cargo => (
            format!("https://crates.io/api/v1/crates/{name}?include="),
            crates::found,
        ),
        Ecosystem::Pypi => (format!("https://pypi.org/pypi/{name}/json"), pypi::found),
    }
}

/// `text` as one URL path segment: everything but unreserved characters is percent-encoded,
/// so `@scope/name` is `%40scope%2Fname`.
fn encode(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
