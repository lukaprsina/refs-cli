use refs_cli::edit::{AddRepo, Edit, Target, add, disable, enable, remove};

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
fn add_creates_a_bare_group_that_is_not_in_the_config_and_remove_takes_it_away_again() {
    let req = AddRepo {
        group: Some("router".into()),
        ..new_repo("https://github.com/solidjs/solid-router")
    };

    let added = add(CONFIG, &req).unwrap();

    assert!(added.contains("\n[groups.router]\n"), "{added}");
    let config = refs_cli::config::parse(&added).unwrap();
    let group = config.groups.get("router").unwrap();
    assert!(group.name.is_none() && group.description.is_none());
    assert_eq!(remove(&added, "solid-router").unwrap(), CONFIG);
}

#[test]
fn applying_an_add_says_which_group_it_created() {
    let req = AddRepo {
        group: Some("router".into()),
        ..new_repo("https://github.com/solidjs/solid-router")
    };
    assert_eq!(
        Edit::Add(&req).apply(CONFIG).unwrap().group_created,
        Some("router".into())
    );

    let existing = AddRepo {
        group: Some("solid".into()),
        ..new_repo("https://github.com/solidjs/solid-router")
    };
    assert_eq!(
        Edit::Add(&existing).apply(CONFIG).unwrap().group_created,
        None
    );
}

#[test]
fn add_rejects_an_id_that_is_taken_and_points_at_the_id_option() {
    let err = add(CONFIG, &new_repo("https://github.com/other/solid.git")).unwrap_err();
    assert_eq!(codes(&err), ["refs::config::id_taken"]);
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
    assert_eq!(codes(&err), ["refs::config::unknown_repo"]);
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

#[test]
fn add_appends_at_the_end_of_the_file_whatever_the_section_order() {
    let text =
        "[repos.z]\nurl = \"https://github.com/o/z\"\n\n[groups.g]\nname = \"G\"\n# trailing\n";
    let added = add(text, &new_repo("https://github.com/o/r")).unwrap();
    assert_eq!(
        added,
        format!("{text}\n[repos.r]\nurl = \"https://github.com/o/r\"\n")
    );
    assert_eq!(remove(&added, "r").unwrap(), text);
}

#[test]
fn a_crlf_file_stays_crlf_through_add_and_remove() {
    let text = CONFIG.replace('\n', "\r\n");
    let added = add(&text, &new_repo("https://github.com/o/r")).unwrap();
    assert!(!added.replace("\r\n", "").contains('\n'), "{added:?}");
    assert_eq!(remove(&added, "r").unwrap(), text);
}

#[test]
fn an_inline_repos_table_is_refused_with_a_hint_not_rewritten() {
    let text = "repos = { a = { url = \"https://github.com/o/a\" } }\n";
    let err = add(text, &new_repo("https://github.com/o/r")).unwrap_err();
    assert_eq!(codes(&err), ["refs::config::unreadable"]);
    let err = remove(text, "a").unwrap_err();
    assert_eq!(codes(&err), ["refs::config::unreadable"]);
}

#[test]
fn remove_cuts_the_table_and_its_own_comments_and_leaves_the_rest_alone() {
    let text = "\
# About the first.
[repos.a]
url = \"https://github.com/o/a\" # why
packages = [
  \"x\",
]

# A note for nobody in particular.

# About b.
[repos.b]
url = \"https://github.com/o/b\"

# About c.
[repos.c]
url = \"https://github.com/o/c\"
";
    assert_eq!(
        remove(text, "b").unwrap(),
        "\
# About the first.
[repos.a]
url = \"https://github.com/o/a\" # why
packages = [
  \"x\",
]

# A note for nobody in particular.

# About c.
[repos.c]
url = \"https://github.com/o/c\"
"
    );
}

#[test]
fn remove_also_drops_a_stub_group_that_add_found_empty() {
    // The one exception to the byte-identical round trip (spec §10): a group with no repos
    // and no description renders nothing, and `remove` cleans it up with the repo.
    let text = "[groups.g]\nname = \"G\"\n";
    let req = AddRepo {
        group: Some("g".into()),
        ..new_repo("https://github.com/o/r")
    };
    let added = add(text, &req).unwrap();
    assert_eq!(remove(&added, "r").unwrap(), "");
}

#[test]
fn disable_marks_a_repo_and_enable_gives_back_the_same_bytes() {
    let disabled = disable(CONFIG, Target::Repo("solid")).unwrap();
    let config = refs_cli::config::parse(&disabled).unwrap();
    assert_eq!(config.repos["solid"].enabled, Some(false));
    assert_eq!(enable(&disabled, Target::Repo("solid")).unwrap(), CONFIG);
}

#[test]
fn disable_and_enable_work_on_a_group() {
    let disabled = disable(CONFIG, Target::Group("solid")).unwrap();
    let config = refs_cli::config::parse(&disabled).unwrap();
    assert_eq!(config.groups["solid"].enabled, Some(false));
    assert_eq!(enable(&disabled, Target::Group("solid")).unwrap(), CONFIG);
}

#[test]
fn disable_and_enable_reject_an_id_that_is_not_in_the_config() {
    let err = disable(CONFIG, Target::Repo("nope")).unwrap_err();
    assert_eq!(codes(&err), ["refs::config::unknown_repo"]);
    let err = enable(CONFIG, Target::Group("nope")).unwrap_err();
    assert_eq!(codes(&err), ["refs::config::unknown_group"]);
}

#[test]
fn enable_removes_a_written_enabled_key_and_disable_overwrites_it() {
    let text = "[repos.a]\nurl = \"https://github.com/o/a\"\nenabled = true # on\nref = \"main\"\n";
    let off = disable(text, Target::Repo("a")).unwrap();
    assert_eq!(off, text.replace("true", "false"));
    let on = enable(text, Target::Repo("a")).unwrap();
    assert_eq!(
        on,
        "[repos.a]\nurl = \"https://github.com/o/a\"\nref = \"main\"\n"
    );
}

#[test]
fn enabling_an_enabled_repo_changes_nothing() {
    assert_eq!(enable(CONFIG, Target::Repo("solid")).unwrap(), CONFIG);
}

#[test]
fn the_round_trip_holds_with_crlf_and_without_a_final_newline() {
    for text in [
        CONFIG.replace('\n', "\r\n"),
        CONFIG.trim_end().to_owned(),
        CONFIG.replace('\n', "\r\n").trim_end().to_owned(),
    ] {
        let off = disable(&text, Target::Repo("solid")).unwrap();
        assert!(refs_cli::config::parse(&off).is_ok());
        assert_eq!(enable(&off, Target::Repo("solid")).unwrap(), text);
    }
}

#[test]
fn a_repo_not_written_as_a_table_is_refused_with_a_hint() {
    let text = "repos = { a = { url = \"https://github.com/o/a\" } }\n";
    for err in [
        remove(text, "a").unwrap_err(),
        disable(text, Target::Repo("a")).unwrap_err(),
        enable(text, Target::Repo("a")).unwrap_err(),
    ] {
        assert_eq!(codes(&err), ["refs::config::unreadable"]);
        assert!(err.to_string().contains("rewrite it as one"), "{err}");
    }
}
