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
        list(&config),
        "\
solid (SolidJS 2.0)
  router     https://github.com/solidjs/solid-router  next  src, docs
  old        https://github.com/o/old  HEAD  all  disabled
off (Off) disabled
  inherited  https://github.com/o/inherited  HEAD  all  disabled
ungrouped
  loose      https://github.com/o/loose  HEAD  all
"
    );
}
