use refs_cli::config::parse;
use std::collections::HashMap;

use refs_cli::list::{Status, list, list_status};
use refs_cli::lock::{Lock, LockedRepo};
use refs_cli::plan::{Cause, CheckoutState};
use refs_cli::source::fake::{FakeSource, Method};
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
        entry("paths", '3'),
        entry("dirty", '4'),
        entry("gone", '5'),
        entry("alien", '6'),
        entry("hollow", '7'),
    ]);
    let stale = |cause, dirty: &[&str]| Stale {
        cause,
        dirty_files: dirty.iter().map(|f| f.to_string()).collect(),
    };
    let states = HashMap::from([
        ("ok".to_string(), InSync),
        ("moved".to_string(), stale(Cause::Commit, &[])),
        ("paths".to_string(), stale(Cause::Paths, &[])),
        ("dirty".to_string(), stale(Cause::Commit, &["x"])),
        ("gone".to_string(), Absent),
        ("alien".to_string(), Foreign),
        ("hollow".to_string(), Dangling),
    ]);
    let status = Status {
        lock: Some(lock),
        states,
    };

    assert_eq!(
        list_status(&config, &status, false),
        "  ok      https://github.com/o/r  HEAD  (whole repo)  1111111  ok
  moved   https://github.com/o/r  HEAD  (whole repo)  2222222  wrong SHA
  paths   https://github.com/o/r  HEAD  (whole repo)  3333333  wrong paths
  dirty   https://github.com/o/r  HEAD  (whole repo)  4444444  wrong SHA, dirty
  gone    https://github.com/o/r  HEAD  (whole repo)  5555555  missing
  new     https://github.com/o/r  HEAD  (whole repo)  -        not locked
  hollow  https://github.com/o/r  HEAD  (whole repo)  7777777  missing
  alien   https://github.com/o/r  HEAD  (whole repo)  6666666  foreign
- off     https://github.com/o/r  HEAD  (whole repo)  -        disabled
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
    let status = Status {
        lock: None,
        states: HashMap::new(),
    };
    assert_eq!(
        list_status(&config, &status, false),
        "  a  https://github.com/o/r  HEAD  (whole repo)  -  not locked
"
    );
}

/// `list --status` as the CLI makes it: the Lock read from `root`, the Checkouts inspected
/// through `source`.
fn status_listing(source: &FakeSource, lock: Lock, config: &str) -> String {
    let config = parse(config).unwrap();
    let root = tempfile::tempdir().unwrap();
    lock.write(&Lock::path(root.path())).unwrap();
    let status = refs_cli::sync::status(source, root.path(), &config).unwrap();
    list_status(&config, &status, false)
}

const TWO_PATHS: &str = r#"
[repos.moved]
url = "https://github.com/o/r"
[repos.paths]
url = "https://github.com/o/r"
paths = ["a"]
[repos.same]
url = "https://github.com/o/r"
"#;

fn locked_at(sha: &str, ids: &[&str]) -> Lock {
    Lock::new(
        ids.iter()
            .map(|id| LockedRepo {
                id: (*id).into(),
                pin: pin(sha),
            })
            .collect(),
    )
}

fn seen(sha: &str, paths: &[&str], dirty: &[&str]) -> Observed {
    Observed::At {
        pin: pin(sha),
        paths: paths.iter().map(|p| p.to_string()).collect(),
        dirty_files: dirty.iter().map(|p| p.to_string()).collect(),
    }
}

#[test]
fn status_says_dirty_where_sync_would_refuse_to_move_the_checkout() {
    let sha = "1".repeat(40);
    let source = FakeSource::new();
    source.seed("moved", seen(&"2".repeat(40), &[], &["x"]));
    source.seed("paths", seen(&sha, &["b"], &["x"]));
    source.seed("same", seen(&sha, &[], &["x"]));
    let out = status_listing(
        &source,
        locked_at(&sha, &["moved", "paths", "same"]),
        TWO_PATHS,
    );
    assert!(out.contains("1111111  wrong SHA, dirty\n"), "{out}");
    assert!(out.contains("1111111  wrong paths, dirty\n"), "{out}");
    // dirty but matching: sync leaves it alone
    assert!(out.contains("1111111  ok\n"), "{out}");
}

#[test]
fn status_never_inspects_a_disabled_repo() {
    let source = FakeSource::new();
    source.fail("off", Method::Inspect, "broken");
    let out = status_listing(
        &source,
        locked_at(&"1".repeat(40), &[]),
        "[repos.off]\nurl = \"https://github.com/o/r\"\nenabled = false\n",
    );
    assert!(out.contains("disabled\n"), "{out}");
}
