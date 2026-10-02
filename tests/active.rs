use refs_cli::active::active;
use refs_cli::config::parse;

/// `(group id, repo ids)` per rendered section, in order; `None` is the ungrouped section.
fn sections(text: &str) -> Vec<(Option<String>, Vec<String>)> {
    let config = parse(text).unwrap();
    active(&config)
        .sections
        .iter()
        .map(|s| {
            (
                s.group.map(|(id, _)| id.to_string()),
                s.repos.iter().map(|r| r.id.to_string()).collect(),
            )
        })
        .collect()
}

fn s(group: Option<&str>, repos: &[&str]) -> (Option<String>, Vec<String>) {
    (
        group.map(String::from),
        repos.iter().map(|r| r.to_string()).collect(),
    )
}

#[test]
fn disabling_a_group_disables_its_repos() {
    let text = r#"
[groups.on]
name = "On"
[groups.off]
name = "Off"
enabled = false

[repos.a]
url = "https://github.com/o/a"
group = "on"
[repos.b]
url = "https://github.com/o/a"
group = "off"
[repos.c]
url = "https://github.com/o/a"
group = "off"
enabled = true
"#;
    assert_eq!(sections(text), [s(Some("on"), &["a"])]);
}

#[test]
fn ungrouped_repos_come_last_and_a_disabled_repo_is_absent() {
    let text = r#"
[groups.g1]
name = "G1"
[groups.g2]
name = "G2"

[repos.loose]
url = "https://github.com/o/a"
[repos.in2]
url = "https://github.com/o/a"
group = "g2"
[repos.in1]
url = "https://github.com/o/a"
group = "g1"
[repos.in1b]
url = "https://github.com/o/a"
group = "g1"
[repos.gone]
url = "https://github.com/o/a"
group = "g1"
enabled = false
[repos.loose-off]
url = "https://github.com/o/a"
enabled = false
"#;
    assert_eq!(
        sections(text),
        [
            s(Some("g1"), &["in1", "in1b"]),
            s(Some("g2"), &["in2"]),
            s(None, &["loose"])
        ]
    );
}

#[test]
fn a_group_without_active_repos_has_no_section() {
    let text = r#"
[groups.empty]
name = "Empty"
[groups.all-off]
name = "All off"
[repos.a]
url = "https://github.com/o/a"
group = "all-off"
enabled = false
"#;
    assert_eq!(sections(text), []);
}
