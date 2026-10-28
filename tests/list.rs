use refs_cli::config::parse;

use refs_cli::list::list;

#[test]
fn lists_repos_under_their_groups_with_ref_paths_and_disabled_marks() {
    let config = parse(
        r#"
[groups.solid]
name = "SolidJS 2.0"

[groups.off]
name = "Off"
enabled = false

[repos.router]
url = "https://github.com/solidjs/solid-router"
group = "solid"
ref = "next"
paths = ["src", "docs"]

[repos.old]
url = "https://github.com/o/old"
group = "solid"
enabled = false

[repos.inherited]
url = "https://github.com/o/inherited"
group = "off"

[repos.loose]
url = "https://github.com/o/loose"
"#,
    )
    .unwrap();
    assert_eq!(
        list(&config, None, false),
        "\
SolidJS 2.0 (solid)
  router     https://github.com/solidjs/solid-router  next  src, docs
- old        https://github.com/o/old                 HEAD  (whole repo)
- Off (off)
- inherited  https://github.com/o/inherited           HEAD  (whole repo)
ungrouped
  loose      https://github.com/o/loose               HEAD  (whole repo)
"
    );
}

#[test]
fn without_groups_there_is_no_heading() {
    let config = parse("[repos.a]\nurl = \"https://github.com/o/r\"\n").unwrap();
    assert_eq!(
        list(&config, None, false),
        "  a  https://github.com/o/r  HEAD  (whole repo)\n"
    );
}

#[test]
fn a_group_heading_is_just_the_name_when_name_and_id_agree() {
    let config = parse(
        "[groups.same]\nname = \"same\"\n[repos.a]\nurl = \"https://github.com/o/r\"\ngroup = \"same\"\n",
    )
    .unwrap();
    assert_eq!(
        list(&config, None, false),
        "same\n  a  https://github.com/o/r  HEAD  (whole repo)\n"
    );
}

#[test]
fn disabled_lines_are_dimmed_only_with_color() {
    let config = parse(
        "[repos.a]\nurl = \"https://github.com/o/r\"\n[repos.b]\nurl = \"https://github.com/o/r\"\nenabled = false\n",
    )
    .unwrap();
    assert_eq!(
        list(&config, None, true),
        "  a  https://github.com/o/r  HEAD  (whole repo)\n\
\x1b[2m- b  https://github.com/o/r  HEAD  (whole repo)\x1b[0m\n"
    );
}
