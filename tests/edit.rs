use refs_cli::edit::{AddRepo, add, remove};

const CONFIG: &str = r#"# My references.

[settings]
agents_files = ["AGENTS.md"] # the one that matters

[groups.solid]
name = "SolidJS"

# The core.
[repos.solid]
url = "https://github.com/solidjs/solid"
group = "solid"
"#;

fn new_repo(url: &str) -> AddRepo {
    AddRepo {
        url: url.into(),
        ..AddRepo::default()
    }
}

#[test]
fn add_then_remove_gives_back_the_same_bytes() {
    let added = add(CONFIG, &new_repo("https://github.com/solidjs/solid-router")).unwrap();
    assert!(added.contains("[repos.solid-router]"), "{added}");
    assert_eq!(remove(&added, "solid-router").unwrap(), CONFIG);
}

#[test]
fn add_writes_every_option_it_is_given() {
    let req = AddRepo {
        url: "https://github.com/solidjs/solid-router".into(),
        id: Some("router".into()),
        group: Some("solid".into()),
        git_ref: Some("next".into()),
        description: Some("Router 2.0 source".into()),
        paths: vec!["src".into()],
        packages: vec!["@solidjs/router".into()],
        start: vec!["README.md".into()],
    };
    let config = refs_cli::config::parse(&add(CONFIG, &req).unwrap()).unwrap();
    let (id, repo) = config.repos.last().unwrap();
    assert_eq!(id.as_ref(), "router");
    assert_eq!(repo.url.as_ref(), "https://github.com/solidjs/solid-router");
    assert_eq!(repo.group.as_ref().unwrap().as_ref(), "solid");
    assert_eq!(repo.git_ref.as_ref().unwrap().as_ref(), "next");
    assert_eq!(
        repo.description.as_ref().unwrap().as_ref(),
        "Router 2.0 source"
    );
    assert_eq!(repo.path_strings(), ["src"]);
    assert_eq!(repo.packages[0].as_ref(), "@solidjs/router");
    assert_eq!(repo.start[0].as_ref(), "README.md");
}

/// The diagnostic codes of an error and of the errors related to it.
fn codes(err: &dyn miette::Diagnostic) -> Vec<String> {
    err.code()
        .map(|c| c.to_string())
        .into_iter()
        .chain(err.related().into_iter().flatten().flat_map(codes))
        .collect()
}

#[test]
fn add_rejects_a_url_or_ref_that_fails_validation() {
    let bad_url = new_repo("http://github.com/solidjs/solid-router");
    let err = add(CONFIG, &bad_url).unwrap_err();
    assert_eq!(codes(&err), ["refs::config::bad_url"]);

    let bad_ref = AddRepo {
        git_ref: Some("-rf".into()),
        ..new_repo("https://github.com/solidjs/solid-router")
    };
    let err = add(CONFIG, &bad_ref).unwrap_err();
    assert_eq!(codes(&err), ["refs::config::bad_ref"]);
}

#[test]
fn add_rejects_a_group_that_is_not_in_the_config() {
    let req = AddRepo {
        group: Some("nope".into()),
        ..new_repo("https://github.com/solidjs/solid-router")
    };
    let err = add(CONFIG, &req).unwrap_err();
    assert_eq!(codes(&err), ["refs::config::dangling_group"]);
}

#[test]
fn add_rejects_an_id_that_is_taken_and_points_at_the_id_option() {
    let err = add(CONFIG, &new_repo("https://github.com/other/solid.git")).unwrap_err();
    assert_eq!(codes(&err), ["refs::edit::id_taken"]);
    assert!(
        miette::Diagnostic::help(&err)
            .unwrap()
            .to_string()
            .contains("--id"),
        "{err:?}"
    );
}

#[test]
fn the_default_id_is_the_last_url_segment_without_dot_git() {
    for (url, id) in [
        ("https://github.com/solidjs/solid-router", "solid-router"),
        (
            "https://github.com/solidjs/solid-router.git",
            "solid-router",
        ),
        ("https://github.com/solidjs/solid-router/", "solid-router"),
        ("git@github.com:solidjs/solid-router.git", "solid-router"),
        ("ssh://git@host/solid-router.git", "solid-router"),
    ] {
        let config = refs_cli::config::parse(&add(CONFIG, &new_repo(url)).unwrap()).unwrap();
        assert_eq!(config.repos.last().unwrap().0.as_ref(), id, "{url}");
    }
}

#[test]
fn remove_rejects_an_id_that_is_not_in_the_config() {
    let err = remove(CONFIG, "nope").unwrap_err();
    assert_eq!(codes(&err), ["refs::edit::unknown_repo"]);
}

mod groups {
    use super::*;

    const WITH_GROUPS: &str = r#"[groups.plain]
name = "Plain"

[groups.described]
name = "Described"
description = "When to look here"

[repos.a]
url = "https://github.com/o/a"
group = "plain"

[repos.b]
url = "https://github.com/o/b"
group = "plain"
enabled = false

[repos.c]
url = "https://github.com/o/c"
group = "described"
"#;

    fn group_ids(text: &str) -> Vec<String> {
        let config = refs_cli::config::parse(text).unwrap();
        config.groups.keys().map(|k| k.as_ref().clone()).collect()
    }

    #[test]
    fn removing_the_last_repo_removes_a_group_without_a_description() {
        let after_a = remove(WITH_GROUPS, "a").unwrap();
        // `b` is disabled but still a member, so the group stays.
        assert_eq!(group_ids(&after_a), ["plain", "described"]);
        let after_b = remove(&after_a, "b").unwrap();
        assert_eq!(group_ids(&after_b), ["described"]);
    }

    #[test]
    fn a_group_with_a_description_is_kept_when_it_empties() {
        let after_c = remove(WITH_GROUPS, "c").unwrap();
        assert_eq!(group_ids(&after_c), ["plain", "described"]);
    }
}

#[test]
fn add_and_remove_round_trip_when_there_are_no_repos_or_no_final_newline() {
    for text in [
        "",
        "# nothing yet\n",
        "[settings]\nreferences_dir = \"refs\"",
        "[groups.g]\nname = \"G\"\n",
    ] {
        let added = add(text, &new_repo("https://github.com/o/r")).unwrap();
        assert!(added.contains("[repos.r]"), "{added:?}");
        assert_eq!(remove(&added, "r").unwrap(), text, "{text:?}");
    }
}
