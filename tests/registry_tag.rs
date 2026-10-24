//! Which tag a package's version maps to, over tag lists recorded from real repositories.

use refs_cli::registry::tag::{newest, split, tag_for};

fn list(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

#[test]
fn a_package_qualified_tag_matches() {
    let tags = list(&["tokio-1.39.0", "tokio-1.40.0", "tokio-util-0.7.12"]);
    assert_eq!(tag_for("tokio", "1.40.0", &tags), Some("tokio-1.40.0"));
    assert_eq!(
        tag_for("tokio-util", "0.7.12", &tags),
        Some("tokio-util-0.7.12")
    );
}

#[test]
fn bare_version_tags_match_last() {
    assert_eq!(
        tag_for("log", "0.4.22", &list(&["0.4.21", "0.4.22"])),
        Some("0.4.22")
    );
    assert_eq!(
        tag_for("serde", "1.0.200", &list(&["v1.0.199", "v1.0.200"])),
        Some("v1.0.200")
    );
    assert_eq!(
        tag_for("x", "1.0.0", &list(&["1.0.0", "v1.0.0"])),
        Some("v1.0.0")
    );
}

#[test]
fn a_scoped_name_matches_by_its_full_name_first() {
    let tags = list(&["@changesets/cli@2.27.0", "@babel/core@7.23.2", "v7.23.2"]);
    assert_eq!(
        tag_for("@changesets/cli", "2.27.0", &tags),
        Some("@changesets/cli@2.27.0")
    );
    assert_eq!(
        tag_for("@babel/core", "7.23.2", &tags),
        Some("@babel/core@7.23.2")
    );
}

#[test]
fn a_scoped_name_falls_back_to_the_part_after_the_slash() {
    let tags = list(&["core@1.0.0", "v1.0.0"]);
    assert_eq!(tag_for("@babel/core", "1.0.0", &tags), Some("core@1.0.0"));
    // The full name is tried with every spelling before the short name is.
    let tags = list(&["core-1.0.0", "@babel/core-1.0.0"]);
    assert_eq!(
        tag_for("@babel/core", "1.0.0", &tags),
        Some("@babel/core-1.0.0")
    );
}

#[test]
fn only_an_exact_version_matches() {
    let tags = list(&[
        "tokio-1.40.0",
        "v1.40.0.post1",
        "release-9.9.9",
        "2.31.0.post1",
    ]);
    assert_eq!(tag_for("tokio", "9.9.9", &tags), None);
    assert_eq!(tag_for("requests", "2.31.0", &tags), None);
}

#[test]
fn no_tags_gives_no_tag() {
    assert_eq!(tag_for("anything", "1.0.0", &[]), None);
}

#[test]
fn a_tag_splits_where_the_rest_is_a_semver_version() {
    for (tag, prefix, version) in [
        ("v19.0.9", "v", "19.0.9"),
        ("1.2.3", "", "1.2.3"),
        ("foo2-1.0.0", "foo2-", "1.0.0"),
        ("@scope/pkg@2.0.0-rc.17", "@scope/pkg@", "2.0.0-rc.17"),
    ] {
        let (got_prefix, got_version) = split(tag).expect(tag);
        assert_eq!(
            (got_prefix, got_version.to_string()),
            (prefix, version.to_owned())
        );
    }
    for tag in ["latest", "v1", "1.2", "release-2024"] {
        assert!(split(tag).is_none(), "{tag}");
    }
}

#[test]
fn the_newest_tag_has_the_same_prefix() {
    let tags = list(&[
        "react-19.0.0",
        "react-19.1.0",
        "react-dom-19.2.0",
        "v20.0.0",
        "react-9.0.0",
    ]);
    assert_eq!(newest("react-19.0.0", &tags), Some("react-19.1.0"));
    assert_eq!(newest("react-dom-19.2.0", &tags), None);
    assert_eq!(newest("v20.0.0", &tags), None);
}

#[test]
fn a_stable_tag_ignores_prereleases_and_a_prerelease_tag_does_not() {
    let tags = list(&["v1.0.0", "v2.0.0-rc.17", "v2.0.0-rc.18", "v2.0.0"]);
    assert_eq!(newest("v1.0.0", &tags), Some("v2.0.0"));
    assert_eq!(newest("v2.0.0-rc.17", &tags), Some("v2.0.0"));
    let tags = list(&["v1.0.0", "v2.0.0-rc.1", "v2.0.0-rc.2"]);
    assert_eq!(newest("v1.0.0", &tags), None);
    assert_eq!(newest("v2.0.0-rc.1", &tags), Some("v2.0.0-rc.2"));
}

#[test]
fn build_metadata_does_not_make_a_tag_newer() {
    let tags = list(&["v1.0.0", "v1.0.0+build", "v1.0.1+a", "v1.0.1+b"]);
    assert_eq!(newest("v1.0.0", &tags), Some("v1.0.1+b"));
    assert_eq!(newest("v1.0.0", &list(&["v1.0.0+build"])), None);
}

#[test]
fn a_tag_with_no_version_has_no_newest() {
    assert_eq!(newest("latest", &list(&["latest", "v1.0.0"])), None);
}
