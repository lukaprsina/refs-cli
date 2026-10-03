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
        paths: vec![],
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
        list_status(&config, &status),
        "\
ungrouped
  ok      https://github.com/o/r  HEAD  all  1111111  ok
  moved   https://github.com/o/r  HEAD  all  2222222  wrong SHA
  gone    https://github.com/o/r  HEAD  all  3333333  missing
  new     https://github.com/o/r  HEAD  all  -  not locked
  hollow  https://github.com/o/r  HEAD  all  5555555  missing
  alien   https://github.com/o/r  HEAD  all  4444444  foreign
  off     https://github.com/o/r  HEAD  all  -  disabled
"
    );
}

#[test]
fn status_without_a_lock_says_not_locked() {
    let config = parse("[repos.a]\nurl = \"https://github.com/o/r\"\n").unwrap();
    let status = Status {
        lock: None,
        observed: HashMap::new(),
    };
    assert_eq!(
        list_status(&config, &status),
        "ungrouped\n  a  https://github.com/o/r  HEAD  all  -  not locked\n"
    );
}
