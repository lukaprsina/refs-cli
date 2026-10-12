//! The real `Registry`: the registries of `Ecosystem::spec` over HTTPS.
//!
//! After changing `get`, run `refs add npm:`, `cargo:` and `pypi:` by hand against the real
//! registries (`--no-sync` is enough).

use std::time::Duration;

use reqwest::blocking::Client;

use super::{Found, Package, Registry, RegistryError};

/// crates.io answers 403 to a request without a User-Agent that says who is asking.
const USER_AGENT: &str = concat!(
    "refs-cli/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/lukaprsina/refs-cli)"
);
const TIMEOUT: Duration = Duration::from_secs(15);

/// Asks npm, crates.io and PyPI over HTTPS. One lookup per `add`, far under crates.io's rate
/// limit of a request per second, so there is no retry. Tests never touch the network: they
/// check `request_url` and `interpret`, and the GET itself is the part to run by hand after a change.
pub struct Http;

impl Registry for Http {
    fn lookup(&self, package: &Package) -> Result<Found, RegistryError> {
        let (status, body) = get(&request_url(package)).map_err(|why| RegistryError::Request {
            package: package.clone(),
            why,
        })?;
        interpret(package, status, &body)
    }
}

fn get(url: &str) -> Result<(u16, String), String> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = response.text().map_err(|e| e.to_string())?;
    Ok((status, body))
}

/// What a registry's answer (`status` and `body`) says about `package`.
pub fn interpret(package: &Package, status: u16, body: &str) -> Result<Found, RegistryError> {
    let package = || package.clone();
    match status {
        404 => return Err(RegistryError::NotFound { package: package() }),
        status if !(200..300).contains(&status) => {
            return Err(RegistryError::Status {
                package: package(),
                status,
            });
        }
        _ => {}
    }
    match (package().ecosystem.spec().found)(body) {
        Ok(Some(found)) => Ok(found),
        Ok(None) => Err(RegistryError::NoRepository { package: package() }),
        Err(why) => Err(RegistryError::Malformed {
            package: package(),
            why,
        }),
    }
}

/// The URL of the document that holds the repository: the cheapest one the registry serves.
pub fn request_url(package: &Package) -> String {
    let spec = package.ecosystem.spec();
    spec.url.replace("{name}", &encode(&package.name))
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
