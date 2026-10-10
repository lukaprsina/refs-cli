//! Which tag a package's version maps to, over tag lists recorded from real repositories.

use refs_cli::registry::tag::tag_for;

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
