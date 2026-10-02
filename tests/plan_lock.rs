use miette::Diagnostic;
use refs_cli::active::active;
use refs_cli::config::parse;
use refs_cli::diagnostic::LockRefusal;
use refs_cli::lock::{Field, Lock, LockedRepo};
use refs_cli::plan::{Drift, LockFlags, Step, Upgrade, lock_drift, plan_lock};
use refs_cli::source::Pin;

const SHA: &str = "ee49b3e0123456789012345678901234567890ab";

fn entry(id: &str, url: &str, git_ref: &str, paths: &[&str]) -> LockedRepo {
    LockedRepo {
        id: id.into(),
        pin: Pin::git(url, git_ref, SHA, None),
        paths: paths.iter().map(|p| p.to_string()).collect(),
    }
}

fn lock(repo: Vec<LockedRepo>) -> Lock {
    Lock { version: 1, repo }
}

const A: &str = r#"
[repos.a]
url = "https://github.com/o/a"
ref = "next"
paths = ["docs"]
"#;

fn a_entry() -> LockedRepo {
    entry("a", "https://github.com/o/a", "next", &["docs"])
}

#[test]
fn a_missing_lock_is_stale_and_every_repo_resolves() {
    let config = parse(A).unwrap();
    let set = active(&config);
    assert_eq!(lock_drift(&set, None), vec![Drift::LockMissing]);
    let plan = plan_lock(&set, None, &LockFlags::default()).unwrap();
    assert_eq!(plan.drift, vec![Drift::LockMissing]);
    assert!(matches!(plan.steps.as_slice(), [Step::Resolve(r)] if r.id == "a"));
}

#[test]
fn a_matching_lock_is_current_and_every_repo_is_reused() {
    let config = parse(A).unwrap();
    let set = active(&config);
    let locked = lock(vec![a_entry()]);
    assert_eq!(lock_drift(&set, Some(&locked)), vec![]);
    let plan = plan_lock(&set, Some(&locked), &LockFlags::default()).unwrap();
    assert_eq!(plan.drift, vec![]);
    assert!(matches!(plan.steps.as_slice(), [Step::Reuse(e)] if *e == a_entry()));
}

#[test]
fn editing_paths_keeps_the_pin_and_refreshes_the_entry_paths() {
    let config = parse(&A.replace(r#"["docs"]"#, r#"["docs", "src"]"#)).unwrap();
    let set = active(&config);
    let locked = lock(vec![a_entry()]);
    let drift = vec![Drift::Changed {
        id: "a".into(),
        field: Field::Paths,
    }];
    assert_eq!(lock_drift(&set, Some(&locked)), drift);
    let plan = plan_lock(&set, Some(&locked), &LockFlags::default()).unwrap();
    assert_eq!(plan.drift, drift);
    let want = entry("a", "https://github.com/o/a", "next", &["docs", "src"]);
    assert!(matches!(plan.steps.as_slice(), [Step::Reuse(e)] if *e == want));
}

fn changed(field: Field) -> Vec<Drift> {
    vec![Drift::Changed {
        id: "a".into(),
        field,
    }]
}

#[test]
fn changing_the_url_or_the_ref_resolves_again() {
    for (text, field) in [
        (A.replace("o/a", "o/b"), Field::Url),
        (A.replace("next", "main"), Field::Ref),
    ] {
        let config = parse(&text).unwrap();
        let set = active(&config);
        let locked = lock(vec![a_entry()]);
        assert_eq!(lock_drift(&set, Some(&locked)), changed(field));
        let plan = plan_lock(&set, Some(&locked), &LockFlags::default()).unwrap();
        assert!(matches!(plan.steps.as_slice(), [Step::Resolve(r)] if r.id == "a"));
    }
}

#[test]
fn an_active_repo_without_an_entry_is_added_and_resolves() {
    // Disabling drops the entry, so a re-enabled repo looks exactly like a new one.
    let config = parse(&format!(
        "{A}\n[repos.b]\nurl = \"https://github.com/o/b\"\n"
    ))
    .unwrap();
    let set = active(&config);
    let locked = lock(vec![a_entry()]);
    assert_eq!(
        lock_drift(&set, Some(&locked)),
        vec![Drift::Added("b".into())]
    );
    let plan = plan_lock(&set, Some(&locked), &LockFlags::default()).unwrap();
    assert!(matches!(
        plan.steps.as_slice(),
        [Step::Reuse(e), Step::Resolve(r)] if e.id == "a" && r.id == "b"
    ));
}

#[test]
fn an_entry_for_a_disabled_repo_is_removed_and_gets_no_step() {
    let config = parse(&format!("{A}enabled = false\n")).unwrap();
    let set = active(&config);
    let locked = lock(vec![a_entry()]);
    let want = vec![Drift::Removed("a".into())];
    assert_eq!(lock_drift(&set, Some(&locked)), want);
    let plan = plan_lock(&set, Some(&locked), &LockFlags::default()).unwrap();
    assert_eq!(plan.drift, want);
    assert!(plan.steps.is_empty());
}

#[test]
fn display_fields_start_and_groups_do_not_stale_the_lock() {
    let config = parse(
        r#"
[groups.g]
name = "G"

[repos.a]
url = "https://github.com/o/a"
ref = "next"
paths = ["docs"]
group = "g"
description = "changed"
packages = ["p"]
start = ["docs"]
"#,
    )
    .unwrap();
    let set = active(&config);
    assert_eq!(lock_drift(&set, Some(&lock(vec![a_entry()]))), vec![]);
}

const THREE: &str = r#"
[repos.branch]
url = "https://github.com/o/branch"
ref = "next"
[repos.pinned]
url = "https://github.com/o/pinned"
ref = "ee49b3e0123456789012345678901234567890ab"
[repos.head]
url = "https://github.com/o/head"
"#;

fn three_locked() -> Lock {
    lock(vec![
        entry("branch", "https://github.com/o/branch", "next", &[]),
        entry(
            "pinned",
            "https://github.com/o/pinned",
            "ee49b3e0123456789012345678901234567890ab",
            &[],
        ),
        entry("head", "https://github.com/o/head", "HEAD", &[]),
    ])
}

/// Which repos resolve, in config order.
fn resolved(upgrade: Upgrade) -> Vec<String> {
    let config = parse(THREE).unwrap();
    let set = active(&config);
    let locked = three_locked();
    let flags = LockFlags {
        upgrade,
        ..LockFlags::default()
    };
    let plan = plan_lock(&set, Some(&locked), &flags).unwrap();
    assert_eq!(plan.drift, vec![]);
    plan.steps
        .iter()
        .filter_map(|s| match s {
            Step::Resolve(r) => Some(r.id.to_string()),
            Step::Reuse(_) => None,
        })
        .collect()
}

#[test]
fn without_upgrade_nothing_floating_moves() {
    assert_eq!(resolved(Upgrade::None), Vec::<String>::new());
}

#[test]
fn upgrade_re_resolves_floating_refs_but_not_a_full_sha() {
    assert_eq!(resolved(Upgrade::All), ["branch", "head"]);
}

#[test]
fn upgrade_with_ids_re_resolves_only_those() {
    assert_eq!(resolved(Upgrade::Ids(vec!["head".into()])), ["head"]);
    assert_eq!(
        resolved(Upgrade::Ids(vec!["pinned".into()])),
        Vec::<String>::new()
    );
}

fn offline() -> LockFlags {
    LockFlags {
        offline: true,
        ..LockFlags::default()
    }
}

#[test]
fn offline_refuses_a_missing_or_stale_lock_naming_the_drift() {
    let config = parse(A).unwrap();
    let set = active(&config);
    let err = plan_lock(&set, None, &offline()).unwrap_err();
    assert_eq!(err.code().unwrap().to_string(), "refs::lock::offline_stale");
    assert!(
        matches!(&err, LockRefusal::OfflineStale { drift } if *drift == vec![Drift::LockMissing])
    );

    let stale = lock(vec![entry(
        "a",
        "https://github.com/o/a",
        "main",
        &["docs"],
    )]);
    let err = plan_lock(&set, Some(&stale), &offline()).unwrap_err();
    assert!(matches!(&err, LockRefusal::OfflineStale { drift } if *drift == changed(Field::Ref)));
}

#[test]
fn offline_with_a_current_lock_plans_normally() {
    let config = parse(A).unwrap();
    let set = active(&config);
    let plan = plan_lock(&set, Some(&lock(vec![a_entry()])), &offline()).unwrap();
    assert!(matches!(plan.steps.as_slice(), [Step::Reuse(_)]));
}

#[test]
fn upgrading_a_repo_that_is_not_active_is_a_refusal() {
    let config = parse(&format!("{A}enabled = false\n")).unwrap();
    let set = active(&config);
    let flags = LockFlags {
        upgrade: Upgrade::Ids(vec!["a".into(), "nope".into()]),
        ..LockFlags::default()
    };
    let err = plan_lock(&set, None, &flags).unwrap_err();
    assert_eq!(err.code().unwrap().to_string(), "refs::lock::unknown_id");
    assert!(matches!(&err, LockRefusal::UnknownUpgradeId { ids } if ids == &["a", "nope"]));
}
