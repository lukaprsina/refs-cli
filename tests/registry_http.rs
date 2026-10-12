//! `Http` without a network: the URL it asks for (`url`), and what each answer becomes
//! (`interpret`). The GET itself (User-Agent, timeout) is checked by hand.

use refs_cli::registry::{Ecosystem, Package, RegistryError, interpret, url};

const NPM_BODY: &str =
    r#"{ "repository": { "url": "git+https://github.com/o/r.git", "directory": "pkg" } }"#;
const PYPI_BODY: &str = r#"{ "info": { "project_urls": { "Source": "https://github.com/o/r" } } }"#;

fn answer(ecosystem: Ecosystem, status: u16, body: &str) -> Result<String, RegistryError> {
    interpret(&Package::new(ecosystem, "x"), status, body).map(|f| f.url)
}

fn ask(ecosystem: Ecosystem, name: &str) -> String {
    url(&Package::new(ecosystem, name))
}

#[test]
fn each_ecosystem_asks_its_cheapest_document() {
    assert_eq!(
        ask(Ecosystem::Npm, "left-pad"),
        "https://registry.npmjs.org/left-pad/latest"
    );
    assert_eq!(
        ask(Ecosystem::Cargo, "serde"),
        "https://crates.io/api/v1/crates/serde?include="
    );
    assert_eq!(
        ask(Ecosystem::Pypi, "requests"),
        "https://pypi.org/pypi/requests/json"
    );
}

#[test]
fn each_ecosystem_reads_its_own_document() {
    assert_eq!(
        answer(Ecosystem::Npm, 200, NPM_BODY).unwrap(),
        "https://github.com/o/r"
    );
    assert_eq!(
        answer(
            Ecosystem::Cargo,
            200,
            r#"{ "crate": { "repository": "https://github.com/serde-rs/serde" } }"#
        )
        .unwrap(),
        "https://github.com/serde-rs/serde"
    );
    assert_eq!(
        answer(Ecosystem::Pypi, 200, PYPI_BODY).unwrap(),
        "https://github.com/o/r"
    );
}

#[test]
fn a_scoped_npm_name_is_one_percent_encoded_path_segment() {
    assert_eq!(
        ask(Ecosystem::Npm, "@babel/core"),
        "https://registry.npmjs.org/%40babel%2Fcore/latest"
    );
}

#[test]
fn a_404_is_not_found() {
    let error = answer(Ecosystem::Pypi, 404, "").unwrap_err();
    assert!(matches!(error, RegistryError::NotFound { .. }), "{error:?}");
}

#[test]
fn another_failing_status_is_reported_with_the_status() {
    let error = answer(Ecosystem::Pypi, 503, "").unwrap_err();
    assert!(
        matches!(error, RegistryError::Status { status: 503, .. }),
        "{error:?}"
    );
}

#[test]
fn a_body_that_is_not_the_document_is_malformed() {
    let error = answer(Ecosystem::Pypi, 200, "<html>").unwrap_err();
    assert!(
        matches!(error, RegistryError::Malformed { .. }),
        "{error:?}"
    );
}

#[test]
fn a_document_without_a_repository_is_no_repository() {
    let error = answer(Ecosystem::Pypi, 200, r#"{ "info": {} }"#).unwrap_err();
    assert!(
        matches!(error, RegistryError::NoRepository { .. }),
        "{error:?}"
    );
}
