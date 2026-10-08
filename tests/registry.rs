// Copyright 2025 Vercel Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file
// except in compliance with the License. You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software distributed under the
// License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
// either express or implied. See the License for the specific language governing permissions
// and limitations under the License.
//
// Modified for refs-cli: see the module comment below.

//! The registry adapters against recorded response bodies: what a package's document says
//! about where it lives, and the URL it is cleaned to. No network.
//!
//! The URL spellings and the host-confusion cases are ported from opensrc
//! (`packages/opensrc/cli/src/core/registries/mod.rs`, Apache-2.0), rewritten as assertions on
//! the adapters; see NOTICE.

use refs_cli::registry::{Found, crates, npm, pypi};

fn found(url: &str, directory: Option<&str>) -> Option<Found> {
    Some(Found {
        url: url.into(),
        directory: directory.map(Into::into),
    })
}

fn npm_with(repository: &str) -> String {
    format!(r#"{{ "name": "x", "version": "1.0.0", "repository": {repository} }}"#)
}

#[test]
fn npm_reads_an_object_repository_and_its_directory() {
    let body = npm_with(
        r#"{ "type": "git", "url": "git+https://github.com/babel/babel.git", "directory": "packages/babel-core" }"#,
    );

    assert_eq!(
        npm::found(&body),
        Ok(found(
            "https://github.com/babel/babel",
            Some("packages/babel-core")
        ))
    );
}

#[test]
fn npm_cleans_each_spelling_of_the_url() {
    for (raw, clean) in [
        (
            "git+https://github.com/lodash/lodash.git",
            "https://github.com/lodash/lodash",
        ),
        (
            "git://github.com/jashkenas/underscore.git",
            "https://github.com/jashkenas/underscore",
        ),
        (
            "git+ssh://git@github.com/Marak/colors.js.git",
            "https://github.com/Marak/colors.js",
        ),
        (
            "git://github.com/retrofox/is-array",
            "https://github.com/retrofox/is-array",
        ),
        (
            "https://github.com/babel/babel.git",
            "https://github.com/babel/babel",
        ),
        ("git@github.com:o/r.git", "https://github.com/o/r"),
        ("https://github.com/o/r/", "https://github.com/o/r"),
        (
            "https://github.com/o/r/tree/main/packages/x",
            "https://github.com/o/r",
        ),
        ("https://github.com/o/r#readme", "https://github.com/o/r"),
        (
            "https://gitlab.com/group/sub/r.git",
            "https://gitlab.com/group/sub/r",
        ),
    ] {
        let body = npm_with(&format!(r#"{{ "url": "{raw}" }}"#));
        assert_eq!(npm::found(&body), Ok(found(clean, None)), "{raw}");
    }
}

#[test]
fn npm_accepts_a_string_repository_in_each_shorthand() {
    for (raw, clean) in [
        ("npm/example", "https://github.com/npm/example"),
        ("github:npm/example", "https://github.com/npm/example"),
        ("gitlab:user/repo", "https://gitlab.com/user/repo"),
        ("bitbucket:user/repo", "https://bitbucket.org/user/repo"),
        ("https://github.com/o/r.git", "https://github.com/o/r"),
    ] {
        let body = npm_with(&format!(r#""{raw}""#));
        assert_eq!(npm::found(&body), Ok(found(clean, None)), "{raw}");
    }
}

#[test]
fn npm_without_a_repository_url_is_no_repository() {
    for body in [
        r#"{ "name": "x" }"#,
        r#"{ "name": "x", "repository": {} }"#,
        r#"{ "name": "x", "repository": { "type": "git" } }"#,
        r#"{ "name": "x", "repository": "" }"#,
    ] {
        assert_eq!(npm::found(body), Ok(None), "{body}");
    }
}

#[test]
fn a_body_that_is_not_json_is_malformed() {
    assert!(npm::found("<html>").is_err());
}

fn crate_with(repository: &str, homepage: &str) -> String {
    format!(
        r#"{{ "crate": {{ "id": "x", "repository": {repository}, "homepage": {homepage} }}, "versions": null }}"#
    )
}

#[test]
fn crates_reads_the_repository_inside_the_crate_object() {
    let body = crate_with(
        r#""https://github.com/serde-rs/serde""#,
        r#""https://serde.rs""#,
    );

    assert_eq!(
        crates::found(&body),
        Ok(found("https://github.com/serde-rs/serde", None))
    );
}

#[test]
fn crates_falls_back_to_a_homepage_only_when_it_is_a_forge_repository() {
    let homepage = crate_with("null", r#""https://github.com/o/r/""#);
    assert_eq!(
        crates::found(&homepage),
        Ok(found("https://github.com/o/r", None))
    );

    for site in [
        "https://serde.rs",
        "https://github.com.attacker.example/o/r",
        "https://example.com/github.com/o/r",
        "https://github.com/o",
    ] {
        let body = crate_with("null", &format!(r#""{site}""#));
        assert_eq!(crates::found(&body), Ok(None), "{site}");
    }
    assert_eq!(crates::found(&crate_with("null", "null")), Ok(None));
}

fn pypi_with(project_urls: &str, home_page: &str) -> String {
    format!(
        r#"{{ "info": {{ "name": "x", "project_urls": {project_urls}, "home_page": {home_page} }} }}"#
    )
}

#[test]
fn pypi_matches_the_source_label_in_any_case() {
    for label in ["Source", "source", "SOURCE CODE", "Repository", "GitHub"] {
        let body = pypi_with(
            &format!(
                r#"{{ "Documentation": "https://x.readthedocs.io", "{label}": "https://github.com/o/r/" }}"#
            ),
            "null",
        );
        assert_eq!(
            pypi::found(&body),
            Ok(found("https://github.com/o/r", None)),
            "{label}"
        );
    }
}

#[test]
fn pypi_prefers_source_to_homepage_and_trusts_a_homepage_only_on_a_forge() {
    let both = pypi_with(
        r#"{ "Homepage": "https://github.com/o/home", "Source": "https://gitlab.com/o/src" }"#,
        "null",
    );
    assert_eq!(
        pypi::found(&both),
        Ok(found("https://gitlab.com/o/src", None))
    );

    let forge = pypi_with(r#"{ "Homepage": "https://github.com/o/r" }"#, "null");
    assert_eq!(
        pypi::found(&forge),
        Ok(found("https://github.com/o/r", None))
    );

    let docs = pypi_with(r#"{ "Homepage": "https://example.org" }"#, "null");
    assert_eq!(pypi::found(&docs), Ok(None));
}

#[test]
fn pypi_falls_back_to_the_legacy_home_page_and_tolerates_nothing_at_all() {
    let legacy = pypi_with("null", r#""https://github.com/o/r""#);
    assert_eq!(
        pypi::found(&legacy),
        Ok(found("https://github.com/o/r", None))
    );

    for body in [
        pypi_with("null", "null"),
        pypi_with("null", r#""""#),
        pypi_with("{}", "null"),
    ] {
        assert_eq!(pypi::found(&body), Ok(None), "{body}");
    }
}

#[test]
fn pypi_does_not_take_a_funding_link_for_the_repository() {
    let body = pypi_with(
        r#"{ "Funding": "https://github.com/sponsors/someone", "Docs": "https://example.org" }"#,
        "null",
    );

    assert_eq!(pypi::found(&body), Ok(None));
}

#[test]
fn a_url_that_is_not_http_or_https_is_not_a_repository() {
    for raw in [
        "file:///etc/passwd",
        "ftp://example.com/o/r",
        "ext::sh -c id",
    ] {
        let body = npm_with(&format!(r#"{{ "url": "{raw}" }}"#));
        assert_eq!(npm::found(&body), Ok(None), "{raw}");
    }
}
