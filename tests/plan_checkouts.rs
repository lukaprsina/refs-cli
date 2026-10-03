use std::collections::HashMap;

use miette::Diagnostic;
use refs_cli::active::{ActiveSet, active};
use refs_cli::config::parse;
use refs_cli::diagnostic::Refusal;
use refs_cli::lock::{Lock, LockedRepo};
use refs_cli::plan::{
    Action, AgentFileText, Checkouts, Exclude, Plan, ProjectObserved, plan_checkouts,
};
use refs_cli::render::render;
use refs_cli::source::fake::FakeSource;
use refs_cli::source::{Observed, Pin, Source};

const SHA: &str = "ee49b3e0123456789012345678901234567890ab";
const OTHER_SHA: &str = "1111111111111111111111111111111111111111";

const CONFIG: &str = r#"
[repos.a]
url = "https://github.com/o/a"
ref = "next"
paths = ["docs", "src"]
"#;

fn pin(sha: &str) -> Pin {
    Pin::git("https://github.com/o/a", "next", sha, None)
}

fn lock() -> Lock {
    Lock {
        version: 1,
        repo: vec![LockedRepo {
            id: "a".into(),
            pin: pin(SHA),
            paths: vec!["docs".into(), "src".into()],
        }],
    }
}

fn at(sha: &str, paths: &[&str], dirty: &[&str]) -> Observed {
    Observed::At {
        pin: pin(sha),
        paths: paths.iter().map(|p| p.to_string()).collect(),
        dirty_files: dirty.iter().map(|p| p.to_string()).collect(),
    }
}

/// What `sync` would read: every active repo and every `extra` name in the references
/// directory, inspected.
fn observe(source: &FakeSource, set: &ActiveSet, extra: &[&str]) -> Checkouts {
    let seen = set
        .repos()
        .map(|r| r.id)
        .chain(extra.iter().copied())
        .map(|id| (id.to_string(), source.inspect(id).unwrap()));
    Checkouts::new(set, seen).unwrap()
}

/// The agent file as it is after a clean `sync`.
fn synced_text(set: &ActiveSet) -> String {
    format!("{}\n", render(set, &lock(), ".references").unwrap())
}

fn project(set: &ActiveSet) -> ProjectObserved {
    ProjectObserved {
        references_dir: ".references".into(),
        agent_files: vec![AgentFileText {
            path: "AGENTS.md".into(),
            text: Some(synced_text(set)),
        }],
        exclude: Exclude::Present,
    }
}

fn plan(source: &FakeSource, project: &ProjectObserved, force: bool) -> Plan {
    plan_with(source, &[], project, force)
}

/// As `plan`, with `extra` names found in the references directory.
fn plan_with(source: &FakeSource, extra: &[&str], project: &ProjectObserved, force: bool) -> Plan {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let observed = observe(source, &set, extra);
    plan_checkouts(&set, &lock(), &observed, project, force).unwrap()
}

fn in_sync_source() -> FakeSource {
    let source = FakeSource::new();
    source.seed("a", at(SHA, &["docs", "src"], &[]));
    source
}

#[test]
fn an_in_sync_project_has_an_empty_plan() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let plan = plan(&in_sync_source(), &project(&set), false);
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert!(!plan.is_drift());
}

#[test]
fn an_absent_checkout_is_materialised_at_the_locked_pin() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let plan = plan(&FakeSource::new(), &project(&set), false);
    assert!(matches!(
        plan.actions.as_slice(),
        [Action::Materialise { id, pin: p }] if id == "a" && *p == pin(SHA)
    ));
    assert!(plan.is_drift());
}

fn materialised(plan: &Plan) -> bool {
    matches!(plan.actions.as_slice(), [Action::Materialise { id, .. }] if id == "a")
}

#[test]
fn a_checkout_at_another_commit_or_other_paths_is_materialised() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    for observed in [
        at(OTHER_SHA, &["docs", "src"], &[]),
        at(SHA, &["docs"], &[]),
        at(SHA, &[], &[]),
    ] {
        let source = FakeSource::new();
        source.seed("a", observed.clone());
        assert!(
            materialised(&plan(&source, &project(&set), false)),
            "{observed:?}"
        );
    }
}

#[test]
fn paths_are_compared_as_sets() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", at(SHA, &["src", "docs"], &[]));
    assert!(plan(&source, &project(&set), false).actions.is_empty());
}

#[test]
fn no_paths_in_the_config_equals_no_sparse_patterns_on_disk() {
    let config = parse(&CONFIG.replace(r#"paths = ["docs", "src"]"#, "")).unwrap();
    let set = active(&config);
    let mut locked = lock();
    locked.repo[0].paths = vec![];
    let source = FakeSource::new();
    source.seed("a", at(SHA, &[], &[]));
    let text = format!("{}\n", render(&set, &locked, ".references").unwrap());
    let mut project = project(&set);
    project.agent_files[0].text = Some(text);
    let observed = observe(&source, &set, &[]);
    let plan = plan_checkouts(&set, &locked, &observed, &project, false).unwrap();
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
}

#[test]
fn a_pin_differing_only_in_branch_is_in_sync() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed(
        "a",
        Observed::At {
            pin: Pin::git("https://github.com/o/a", "next", SHA, Some("main")),
            paths: vec!["docs".into(), "src".into()],
            dirty_files: vec![],
        },
    );
    assert!(plan(&source, &project(&set), false).actions.is_empty());
}

fn code(d: &dyn Diagnostic) -> String {
    d.code().unwrap().to_string()
}

#[test]
fn a_dangling_checkout_is_recreated_with_a_note_and_force_changes_nothing() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    for force in [false, true] {
        let source = FakeSource::new();
        source.seed("a", Observed::Dangling);
        let plan = plan(&source, &project(&set), force);
        assert!(
            matches!(
                plan.actions.as_slice(),
                [
                    Action::Remove { id: r },
                    Action::Materialise { id: m, .. },
                    Action::Note(note),
                ] if r == "a" && m == "a" && code(note) == "refs::sync::recreated"
            ),
            "{:?}",
            plan.actions
        );
        assert!(plan.is_drift());
    }
}

#[test]
fn a_foreign_directory_is_refused_even_with_force() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    for force in [false, true] {
        let source = FakeSource::new();
        source.seed("a", Observed::Foreign);
        let plan = plan(&source, &project(&set), force);
        assert!(
            matches!(
                plan.actions.as_slice(),
                [Action::Refuse(r @ Refusal::ForeignDir { id })]
                    if id == "a" && code(r) == "refs::sync::foreign_dir"
            ),
            "{:?}",
            plan.actions
        );
    }
}

#[test]
fn a_dirty_checkout_that_must_move_is_refused_naming_the_files() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", at(OTHER_SHA, &["docs", "src"], &["docs/x.md", "junk"]));
    let plan = plan(&source, &project(&set), false);
    assert!(
        matches!(
            plan.actions.as_slice(),
            [Action::Refuse(r @ Refusal::DirtyCheckout { id, files })]
                if id == "a"
                    && files == &["docs/x.md", "junk"]
                    && code(r) == "refs::sync::dirty_checkout"
        ),
        "{:?}",
        plan.actions
    );
    assert!(plan.refusals().count() == 1);
}

#[test]
fn force_replaces_a_dirty_refusal_by_remove_and_materialise() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", at(OTHER_SHA, &["docs", "src"], &["junk"]));
    let plan = plan(&source, &project(&set), true);
    assert!(
        matches!(
            plan.actions.as_slice(),
            [Action::Remove { id: r }, Action::Materialise { id: m, .. }] if r == "a" && m == "a"
        ),
        "{:?}",
        plan.actions
    );
}

#[test]
fn a_dirty_checkout_already_in_sync_is_left_alone() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", at(SHA, &["docs", "src"], &["junk"]));
    assert!(plan(&source, &project(&set), false).actions.is_empty());
}

#[test]
fn a_non_active_checkout_is_removed_before_anything_is_materialised() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("old", at(OTHER_SHA, &[], &[]));
    let plan = plan_with(&source, &["old"], &project(&set), false);
    assert!(
        matches!(
            plan.actions.as_slice(),
            [Action::Remove { id }, Action::Materialise { id: m, .. }] if id == "old" && m == "a"
        ),
        "{:?}",
        plan.actions
    );
}

#[test]
fn a_dirty_non_active_checkout_is_refused_unless_forced() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", at(SHA, &["docs", "src"], &[]));
    source.seed("old", at(OTHER_SHA, &[], &["node_modules"]));
    let project = project(&set);
    let first = plan_with(&source, &["old"], &project, false);
    assert!(
        matches!(
            first.actions.as_slice(),
            [Action::Refuse(Refusal::DirtyCheckout { id, files })]
                if id == "old" && files == &["node_modules"]
        ),
        "{:?}",
        first.actions
    );
    let forced = plan_with(&source, &["old"], &project, true);
    assert!(matches!(forced.actions.as_slice(), [Action::Remove { id }] if id == "old"));
}

#[test]
fn a_foreign_or_absent_non_active_name_is_ignored() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = in_sync_source();
    source.seed("notes", Observed::Foreign);
    let project = project(&set);
    assert!(
        plan_with(&source, &["notes", "gone"], &project, false)
            .actions
            .is_empty()
    );
}

fn with_agent_files(set: &ActiveSet, files: &[(&str, Option<&str>)]) -> ProjectObserved {
    let mut project = project(set);
    project.agent_files = files
        .iter()
        .map(|(path, text)| AgentFileText {
            path: path.to_string(),
            text: text.map(String::from),
        })
        .collect();
    project
}

#[test]
fn a_stale_block_is_rewritten_keeping_the_text_around_it() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let stale = "# Notes\n\n<!-- BEGIN:refs -->\nold\n<!-- END:refs -->\n\nafter\n";
    let project = with_agent_files(&set, &[("AGENTS.md", Some(stale)), ("CLAUDE.md", None)]);
    let plan = plan(&in_sync_source(), &project, false);
    let block = render(&set, &lock(), ".references").unwrap();
    let [
        Action::WriteAgentFile { path: p1, text: t1 },
        Action::WriteAgentFile { path: p2, text: t2 },
    ] = plan.actions.as_slice()
    else {
        panic!("{:?}", plan.actions);
    };
    assert_eq!(p1, "AGENTS.md");
    assert_eq!(*t1, format!("# Notes\n\n{block}\n\nafter\n"));
    assert_eq!(p2, "CLAUDE.md");
    assert_eq!(*t2, format!("{block}\n"));
}

#[test]
fn malformed_markers_are_refused_and_suppress_every_agent_file_write() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let project = with_agent_files(
        &set,
        &[
            ("AGENTS.md", Some("<!-- BEGIN:refs -->\n")),
            ("CLAUDE.md", None),
        ],
    );
    let plan = plan(&in_sync_source(), &project, false);
    assert!(
        matches!(
            plan.actions.as_slice(),
            [Action::Refuse(r @ Refusal::Block { path, .. })]
                if path == "AGENTS.md" && code(r) == "refs::sync::bad_markers"
        ),
        "{:?}",
        plan.actions
    );
}

#[test]
fn a_refusal_suppresses_the_block_write_but_not_the_exclude_rule() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", Observed::Foreign);
    let mut project = with_agent_files(&set, &[("AGENTS.md", None)]);
    project.exclude = Exclude::Missing;
    let plan = plan(&source, &project, false);
    assert!(
        matches!(
            plan.actions.as_slice(),
            [
                Action::Refuse(Refusal::ForeignDir { .. }),
                Action::EnsureExclude
            ]
        ),
        "{:?}",
        plan.actions
    );
}

#[test]
fn a_missing_exclude_rule_is_ensured_last() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let mut project = with_agent_files(&set, &[("AGENTS.md", None)]);
    project.exclude = Exclude::Missing;
    let plan = plan(&FakeSource::new(), &project, false);
    assert!(
        matches!(
            plan.actions.as_slice(),
            [
                Action::Materialise { .. },
                Action::WriteAgentFile { .. },
                Action::EnsureExclude,
            ]
        ),
        "{:?}",
        plan.actions
    );
}

#[test]
fn no_git_repo_is_a_note_and_not_drift() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let mut project = project(&set);
    project.exclude = Exclude::NoGit;
    let plan = plan(&in_sync_source(), &project, false);
    assert!(
        matches!(
            plan.actions.as_slice(),
            [Action::Note(n)] if code(n) == "refs::sync::no_git_repo"
        ),
        "{:?}",
        plan.actions
    );
    assert!(!plan.is_drift());
}

#[test]
fn applying_a_plan_to_the_fake_and_replanning_gives_an_empty_plan() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("old", at(OTHER_SHA, &[], &[]));
    let mut project = with_agent_files(&set, &[("AGENTS.md", None)]);
    let first = plan_with(&source, &["old"], &project, false);
    assert!(first.is_drift());
    for action in &first.actions {
        match action {
            Action::Remove { id } => source.remove(id).unwrap(),
            Action::Materialise { id, pin } => {
                let repo = set.get(id).unwrap();
                source.materialise(repo, pin, Default::default()).unwrap()
            }
            Action::WriteAgentFile { path, text } => {
                project
                    .agent_files
                    .iter_mut()
                    .find(|f| f.path == *path)
                    .unwrap()
                    .text = Some(text.clone())
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(plan(&source, &project, false).actions.is_empty());
}

#[test]
fn a_repo_missing_from_the_lock_is_an_error() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let empty = Lock {
        version: 1,
        repo: vec![],
    };
    let err = plan_checkouts(
        &set,
        &empty,
        &observe(&FakeSource::new(), &set, &[]),
        &project(&set),
        false,
    )
    .unwrap_err();
    assert_eq!(err.ids, vec!["a".to_string()]);
}

#[test]
fn force_leaves_the_clean_rows_unchanged() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    for (observed, drift) in [
        (Observed::Absent, true),
        (at(OTHER_SHA, &["docs", "src"], &[]), true),
        (at(SHA, &["docs", "src"], &[]), false),
    ] {
        let source = FakeSource::new();
        source.seed("a", observed);
        let project = project(&set);
        let plain = plan(&source, &project, false);
        let forced = plan_with(&source, &["old"], &project, true);
        assert_eq!(plain.is_drift(), drift);
        assert_eq!(
            format!("{:?}", plain.actions),
            format!("{:?}", forced.actions)
        );
    }
}

#[test]
fn a_refusal_is_drift() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", Observed::Foreign);
    let plan = plan(&source, &project(&set), false);
    assert!(plan.is_drift());
    assert_eq!(plan.refusals().count(), 1);
}

#[test]
fn a_dangling_non_active_name_is_left_for_doctor() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = in_sync_source();
    source.seed("old", Observed::Dangling);
    let project = project(&set);
    assert!(
        plan_with(&source, &["old"], &project, false)
            .actions
            .is_empty()
    );
}

#[test]
fn an_observation_missing_an_active_repo_is_an_error() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let err = Checkouts::new(&set, HashMap::<String, Observed>::new()).unwrap_err();
    assert_eq!(err.ids, vec!["a".to_string()]);
}

#[test]
fn a_name_that_is_not_active_does_not_satisfy_an_active_repo() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let seen = [("other".to_string(), Observed::Absent)];
    assert!(Checkouts::new(&set, seen).is_err());
}
