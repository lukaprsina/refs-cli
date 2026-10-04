use refs_cli::config::parse;
use std::collections::HashMap;

use refs_cli::list::{Status, list, list_status};
use refs_cli::lock::{Lock, LockedRepo};
use refs_cli::source::{Observed, Pin};

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

fn pin(sha: &str) -> Pin {
    Pin::git("https://github.com/o/r", "HEAD", sha, None)
}

#[test]
fn status_adds_the_locked_sha_and_the_state_of_each_checkout() {
    let config = parse(
        r#"
[repos.ok]
url = "https://github.com/o/r"
[repos.moved]
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
    let sha = |c: char| c.to_string().repeat(40);
    let entry = |id: &str, c| LockedRepo {
        id: id.into(),
        pin: pin(&sha(c)),
    };
    let lock = Lock::new(vec![
        entry("ok", '1'),
        entry("moved", '2'),
        entry("gone", '3'),
        entry("alien", '4'),
        entry("hollow", '5'),
    ]);
    let at = |c| Observed::At {
        pin: pin(&sha(c)),
        paths: vec![],
        dirty_files: vec![],
    };
    let observed = HashMap::from([
        ("ok".to_string(), at('1')),
        ("moved".to_string(), at('9')),
        ("gone".to_string(), Observed::Absent),
        ("alien".to_string(), Observed::Foreign),
        ("hollow".to_string(), Observed::Dangling),
    ]);
    let status = Status {
        lock: Some(lock),
        observed,
    };

    assert_eq!(
        list_status(&config, &status, false),
        "  ok      https://github.com/o/r  HEAD  (whole repo)  1111111  ok
  moved   https://github.com/o/r  HEAD  (whole repo)  2222222  wrong SHA
  gone    https://github.com/o/r  HEAD  (whole repo)  3333333  missing
  new     https://github.com/o/r  HEAD  (whole repo)  -        not locked
  hollow  https://github.com/o/r  HEAD  (whole repo)  5555555  missing
  alien   https://github.com/o/r  HEAD  (whole repo)  4444444  foreign
- off     https://github.com/o/r  HEAD  (whole repo)  -        disabled
"
    );
}

#[test]
fn status_compares_paths_as_a_set() {
    let config = parse(
        r#"
[repos.same]
url = "https://github.com/o/r"
paths = ["b", "a", "b"]
[repos.other]
url = "https://github.com/o/r"
paths = ["a"]
"#,
    )
    .unwrap();
    let sha = "1".repeat(40);
    let entry = |id: &str| LockedRepo {
        id: id.into(),
        pin: pin(&sha),
    };
    let at = |paths: &[&str]| Observed::At {
        pin: pin(&sha),
        paths: paths.iter().map(|p| p.to_string()).collect(),
        dirty_files: vec![],
    };
    let status = Status {
        lock: Some(Lock::new(vec![entry("same"), entry("other")])),
        observed: HashMap::from([
            ("same".to_string(), at(&["a", "b"])),
            ("other".to_string(), at(&["a", "b"])),
        ]),
    };

    let out = list_status(&config, &status, false);
    assert!(out.contains("1111111  ok\n"), "{out}");
    assert!(out.contains("1111111  wrong paths\n"), "{out}");
}

#[test]
fn status_without_a_lock_says_not_locked() {
    let config = parse("[repos.a]\nurl = \"https://github.com/o/r\"\n").unwrap();
    let status = Status {
        lock: None,
        observed: HashMap::new(),
    };
    assert_eq!(
        list_status(&config, &status, false),
        "  a  https://github.com/o/r  HEAD  (whole repo)  -  not locked\n"
    );
}
