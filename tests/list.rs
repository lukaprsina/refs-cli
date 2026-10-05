use refs_cli::config::parse;

use refs_cli::active::LaidRepo;
use refs_cli::list::{list, list_status};
use refs_cli::plan::{Cause, CheckoutState};
use refs_cli::status::{Kind, Row};

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
        list(&config, false),
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
        list(&config, false),
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
        list(&config, false),
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
        list(&config, true),
        "  a  https://github.com/o/r  HEAD  (whole repo)\n\
\x1b[2m- b  https://github.com/o/r  HEAD  (whole repo)\x1b[0m\n"
    );
}

#[test]
fn status_adds_the_locked_sha_and_the_label_of_each_checkout() {
    use CheckoutState::{Absent, Dangling, Foreign, InSync, Stale};
    let config = parse(
        r#"
[repos.ok]
url = "https://github.com/o/r"
[repos.moved]
url = "https://github.com/o/r"
[repos.paths]
url = "https://github.com/o/r"
[repos.dirty]
url = "https://github.com/o/r"
[repos.dirtypaths]
url = "https://github.com/o/r"
[repos.gone]
url = "https://github.com/o/r"
[repos.new]
url = "https://github.com/o/r"
[repos.hollow]
url = "https://github.com/o/r"
[repos.alien]
url = "https://github.com/o/r"
[repos.off]
url = "https://github.com/o/r"
enabled = false
"#,
    )
    .unwrap();
    let stale = |cause, dirty: &[&str]| Stale {
        cause,
        dirty_files: dirty.iter().map(|f| f.to_string()).collect(),
    };
    let row = |c: char, state| Row {
        sha: Some(c.to_string().repeat(7)),
        kind: Kind::Checkout(state),
    };
    let bare = |kind| Row { sha: None, kind };
    let row_of = |laid: &LaidRepo| match laid.repo.id {
        "ok" => row('1', InSync),
        "moved" => row('2', stale(Cause::Commit, &[])),
        "paths" => row('3', stale(Cause::Paths, &[])),
        "dirty" => row('4', stale(Cause::Commit, &["x"])),
        "dirtypaths" => row('8', stale(Cause::Paths, &["x"])),
        "gone" => row('5', Absent),
        "new" => bare(Kind::NotLocked),
        "hollow" => row('7', Dangling),
        "alien" => row('6', Foreign),
        "off" => bare(Kind::Disabled),
        id => panic!("no row for {id}"),
    };

    assert_eq!(
        list_status(&config, row_of, false),
        "  ok          https://github.com/o/r  HEAD  (whole repo)  1111111  ok
  moved       https://github.com/o/r  HEAD  (whole repo)  2222222  wrong SHA
  paths       https://github.com/o/r  HEAD  (whole repo)  3333333  wrong paths
  dirty       https://github.com/o/r  HEAD  (whole repo)  4444444  wrong SHA, dirty
  dirtypaths  https://github.com/o/r  HEAD  (whole repo)  8888888  wrong paths, dirty
  gone        https://github.com/o/r  HEAD  (whole repo)  5555555  missing
  new         https://github.com/o/r  HEAD  (whole repo)  -        not locked
  hollow      https://github.com/o/r  HEAD  (whole repo)  7777777  missing
  alien       https://github.com/o/r  HEAD  (whole repo)  6666666  foreign
- off         https://github.com/o/r  HEAD  (whole repo)  -        disabled
"
    );
}

#[test]
fn status_without_a_lock_says_not_locked() {
    let config = parse(
        "[repos.a]
url = \"https://github.com/o/r\"
",
    )
    .unwrap();
    let row_of = |_: &LaidRepo| Row {
        sha: None,
        kind: Kind::NotLocked,
    };
    assert_eq!(
        list_status(&config, row_of, false),
        "  a  https://github.com/o/r  HEAD  (whole repo)  -  not locked
"
    );
}
