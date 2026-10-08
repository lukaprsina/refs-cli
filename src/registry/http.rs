//! The real `Registry`: the three registries over HTTPS, through a `Fetch` so that tests can
//! script the answers.
//!
//! After changing `Reqwest`, run `refs add npm:`, `cargo:` and `pypi:` by hand against the real
//! registries (`--no-sync` is enough).

use std::time::Duration;

use reqwest::blocking::Client;

use super::{Answer, Ecosystem, Found, Registry, RegistryError, crates, npm, pypi};

/// crates.io answers 403 to a request without a User-Agent that says who is asking.
const USER_AGENT: &str = concat!(
    "refs-cli/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/lukaprsina/refs-cli)"
);
const TIMEOUT: Duration = Duration::from_secs(15);

/// What a registry answered to a GET.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: String,
}

/// One GET, the seam under `Http`. `Err` says why the registry could not be reached or read.
pub trait Fetch {
    fn get(&self, url: &str) -> Result<Reply, String>;
}

impl<T: Fetch> Fetch for &T {
    fn get(&self, url: &str) -> Result<Reply, String> {
        (**self).get(url)
    }
}

/// `Fetch` over HTTPS. It has no test, as tests never touch the network; it is the part of
/// this module to run by hand after a change.
pub struct Reqwest;

impl Fetch for Reqwest {
    fn get(&self, url: &str) -> Result<Reply, String> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| e.to_string())?;
        let response = client.get(url).send().map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let body = response.text().map_err(|e| e.to_string())?;
        Ok(Reply { status, body })
    }
}

/// Asks npm, crates.io and PyPI through `fetch`. One lookup per `add`, far under crates.io's
/// rate limit of a request per second, so there is no retry.
pub struct Http<F>(F);

impl<F: Fetch> Http<F> {
    pub fn new(fetch: F) -> Self {
        Http(fetch)
    }
}

impl<F: Fetch> Registry for Http<F> {
    fn lookup(&self, ecosystem: Ecosystem, name: &str) -> Result<Found, RegistryError> {
        let name_owned = || name.to_owned();
        let (url, read) = endpoint(ecosystem, name);
        let reply = self.0.get(&url).map_err(|why| RegistryError::Request {
            ecosystem,
            name: name_owned(),
            why,
        })?;
        match reply.status {
            404 => {
                return Err(RegistryError::NotFound {
                    ecosystem,
                    name: name_owned(),
                });
            }
            status if !(200..300).contains(&status) => {
                return Err(RegistryError::Status {
                    ecosystem,
                    name: name_owned(),
                    status,
                });
            }
            _ => {}
        }
        match read(&reply.body) {
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
