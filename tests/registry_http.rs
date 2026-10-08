//! `Http` against a scripted `Fetch`: the URL it asks for, and what each answer becomes. No
//! network. The reqwest side (`Reqwest`: User-Agent, timeout) is checked by hand.

use refs_cli::registry::fake::FakeFetch;
use refs_cli::registry::{Ecosystem, Http, Registry, RegistryError};

const NPM_BODY: &str =
    r#"{ "repository": { "url": "git+https://github.com/o/r.git", "directory": "pkg" } }"#;
const PYPI_X: &str = "https://pypi.org/pypi/x/json";

fn ask(fetch: &FakeFetch, ecosystem: Ecosystem, name: &str) -> Result<String, RegistryError> {
    Http::new(fetch).lookup(ecosystem, name).map(|f| f.url)
}

#[test]
fn each_ecosystem_asks_its_cheapest_document() {
    let fetch = FakeFetch::new()
        .replying("https://registry.npmjs.org/left-pad/latest", 200, NPM_BODY)
        .replying(
            "https://crates.io/api/v1/crates/serde?include=",
            200,
            r#"{ "crate": { "repository": "https://github.com/serde-rs/serde" } }"#,
        )
        .replying(
            "https://pypi.org/pypi/requests/json",
            200,
            r#"{ "info": { "project_urls": { "Source": "https://github.com/psf/requests" } } }"#,
        );
    assert_eq!(
        ask(&fetch, Ecosystem::Npm, "left-pad").unwrap(),
        "https://github.com/o/r"
    );
    assert_eq!(
        ask(&fetch, Ecosystem::Cargo, "serde").unwrap(),
        "https://github.com/serde-rs/serde"
    );
    assert_eq!(
        ask(&fetch, Ecosystem::Pypi, "requests").unwrap(),
        "https://github.com/psf/requests"
    );
    assert_eq!(fetch.asked().len(), 3);
}

#[test]
fn a_scoped_npm_name_is_one_percent_encoded_path_segment() {
    let url = "https://registry.npmjs.org/%40babel%2Fcore/latest";
    let fetch = FakeFetch::new().replying(url, 200, NPM_BODY);
    ask(&fetch, Ecosystem::Npm, "@babel/core").unwrap();
    assert_eq!(fetch.asked(), [url]);
}

#[test]
fn a_404_is_not_found() {
    let fetch = FakeFetch::new().replying(PYPI_X, 404, "");
    let error = ask(&fetch, Ecosystem::Pypi, "x").unwrap_err();
    assert!(matches!(error, RegistryError::NotFound { .. }), "{error:?}");
}

#[test]
fn another_failing_status_is_reported_with_the_status() {
    let fetch = FakeFetch::new().replying(PYPI_X, 503, "");
    let error = ask(&fetch, Ecosystem::Pypi, "x").unwrap_err();
    assert!(
        matches!(error, RegistryError::Status { status: 503, .. }),
        "{error:?}"
    );
}

#[test]
fn an_unreachable_registry_is_a_request_error_with_the_reason() {
    let fetch = FakeFetch::new().failing(PYPI_X, "timed out");
    match ask(&fetch, Ecosystem::Pypi, "x").unwrap_err() {
        RegistryError::Request { why, .. } => assert_eq!(why, "timed out"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_body_that_is_not_the_document_is_malformed() {
    let fetch = FakeFetch::new().replying(PYPI_X, 200, "<html>");
    let error = ask(&fetch, Ecosystem::Pypi, "x").unwrap_err();
    assert!(
        matches!(error, RegistryError::Malformed { .. }),
        "{error:?}"
    );
}

#[test]
fn a_document_without_a_repository_is_no_repository() {
    let fetch = FakeFetch::new().replying(PYPI_X, 200, r#"{ "info": {} }"#);
    let error = ask(&fetch, Ecosystem::Pypi, "x").unwrap_err();
    assert!(
        matches!(error, RegistryError::NoRepository { .. }),
        "{error:?}"
    );
}
