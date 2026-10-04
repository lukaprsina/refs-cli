use miette::Diagnostic;
use refs_cli::active::{ActiveSet, active};
use refs_cli::config::{Config, parse};
use refs_cli::diagnostic::Refusal;
use refs_cli::lock::{Lock, LockedRepo};
use refs_cli::plan::{
    AgentFileText, Checkout, Checkouts, Exclude, ExcludeAction, Outcome, Plan, ProjectObserved,
    RepoAction, check_outcome, plan_checkouts,
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
    let listing: Vec<String> = extra.iter().map(|n| n.to_string()).collect();
    Checkouts::observe(set, &listing, |name| source.inspect(name)).unwrap()
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

fn plan(source: &FakeSource, project: &ProjectObserved, force: bool) -> Plan<'static> {
    plan_with(source, &[], project, force)
}

/// As `plan`, with `extra` names found in the references directory.
fn plan_with(
    source: &FakeSource,
    extra: &[&str],
    project: &ProjectObserved,
    force: bool,
) -> Plan<'static> {
    // a Plan borrows the config its Repos come from; leaking it keeps the helper simple
    let config: &'static Config = Box::leak(Box::new(parse(CONFIG).unwrap()));
    let set = active(config);
    let observed = observe(source, &set, extra);
    plan_checkouts(&set, &lock(), &observed, project, force).unwrap()
}

/// Only `exclude` is set, and to `exclude`.
fn empty_but_for(plan: &Plan, exclude: ExcludeAction) -> bool {
    plan.repos.is_empty()
        && plan.writes.is_empty()
        && plan.refusals.is_empty()
        && plan.exclude == Some(exclude)
}

/// Nothing to do, refuse or say.
fn empty(plan: &Plan) -> bool {
    plan.repos.is_empty()
        && plan.writes.is_empty()
        && plan.refusals.is_empty()
        && plan.exclude.is_none()
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
    assert!(empty(&plan), "{plan:?}");
    assert!(!plan.is_drift());
}

#[test]
fn an_absent_checkout_is_materialised_at_the_locked_pin() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let plan = plan(&FakeSource::new(), &project(&set), false);
    assert!(matches!(
        plan.repos.as_slice(),
        [RepoAction::Materialise { repo, pin: p, .. }] if repo.id == "a" && *p == pin(SHA)
    ));
    assert!(plan.is_drift());
}

fn materialised(plan: &Plan) -> bool {
    matches!(plan.repos.as_slice(), [RepoAction::Materialise { repo, .. }] if repo.id == "a")
        && plan.refusals.is_empty()
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
    assert!(empty(&plan(&source, &project(&set), false)));
}

#[test]
fn no_paths_in_the_config_equals_no_sparse_patterns_on_disk() {
    let config = parse(&CONFIG.replace(r#"paths = ["docs", "src"]"#, "")).unwrap();
    let set = active(&config);
    let locked = lock();
    let source = FakeSource::new();
    source.seed("a", at(SHA, &[], &[]));
    let text = format!("{}\n", render(&set, &locked, ".references").unwrap());
    let mut project = project(&set);
    project.agent_files[0].text = Some(text);
    let observed = observe(&source, &set, &[]);
    let plan = plan_checkouts(&set, &locked, &observed, &project, false).unwrap();
    assert!(empty(&plan), "{plan:?}");
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
    assert!(empty(&plan(&source, &project(&set), false)));
}

fn code(d: &dyn Diagnostic) -> String {
    d.code().unwrap().to_string()
}

#[test]
fn a_dangling_checkout_is_replaced_with_a_note_and_force_changes_nothing() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    for force in [false, true] {
        let source = FakeSource::new();
        source.seed("a", Observed::Dangling);
        let plan = plan(&source, &project(&set), force);
        assert!(
            matches!(
                plan.repos.as_slice(),
                [RepoAction::Replace { repo, note: Some(note), .. }]
                    if repo.id == "a" && code(note) == "refs::sync::recreated"
            ) && plan.refusals.is_empty(),
            "{plan:?}"
        );
        assert!(plan.is_drift());
        assert_eq!(plan.repos[0].how(), Checkout::Created);
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
                plan.refusals.as_slice(),
                [r @ Refusal::ForeignDir { id }]
                    if id == "a" && code(r) == "refs::sync::foreign_dir"
            ) && plan.repos.is_empty(),
            "{plan:?}"
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
            plan.refusals.as_slice(),
            [r @ Refusal::DirtyCheckout { id, files }]
                if id == "a"
                    && files == &["docs/x.md", "junk"]
                    && code(r) == "refs::sync::dirty_checkout"
        ) && plan.repos.is_empty(),
        "{plan:?}"
    );
}

#[test]
fn force_replaces_a_dirty_refusal_by_a_replace_without_a_note() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", at(OTHER_SHA, &["docs", "src"], &["junk"]));
    let plan = plan(&source, &project(&set), true);
    assert!(
        matches!(
            plan.repos.as_slice(),
            [RepoAction::Replace { repo, note: None, .. }] if repo.id == "a"
        ) && plan.refusals.is_empty(),
        "{plan:?}"
    );
    assert_eq!(plan.repos[0].how(), Checkout::Moved);
}

#[test]
fn a_dirty_checkout_already_in_sync_is_left_alone() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", at(SHA, &["docs", "src"], &["junk"]));
    assert!(empty(&plan(&source, &project(&set), false)));
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
            plan.repos.as_slice(),
            [RepoAction::Remove { id }, RepoAction::Materialise { repo, .. }]
                if id == "old" && repo.id == "a"
        ),
        "{plan:?}"
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
            first.refusals.as_slice(),
            [Refusal::DirtyCheckout { id, files }]
                if id == "old" && files == &["node_modules"]
        ) && first.repos.is_empty(),
        "{first:?}"
    );
    let forced = plan_with(&source, &["old"], &project, true);
    assert!(matches!(forced.repos.as_slice(), [RepoAction::Remove { id }] if id == "old"));
}

#[test]
fn a_foreign_or_absent_non_active_name_is_ignored() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = in_sync_source();
    source.seed("notes", Observed::Foreign);
    let project = project(&set);
    assert!(empty(&plan_with(
        &source,
        &["notes", "gone"],
        &project,
        false
    )));
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
    let [first, second] = plan.writes.as_slice() else {
        panic!("{plan:?}");
    };
    assert_eq!(first.path, "AGENTS.md");
    assert_eq!(first.text, format!("# Notes\n\n{block}\n\nafter\n"));
    assert_eq!(second.path, "CLAUDE.md");
    assert_eq!(second.text, format!("{block}\n"));
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
            plan.refusals.as_slice(),
            [r @ Refusal::Block { path, .. }]
                if path == "AGENTS.md" && code(r) == "refs::sync::bad_markers"
        ) && plan.writes.is_empty(),
        "{plan:?}"
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
        matches!(plan.refusals.as_slice(), [Refusal::ForeignDir { .. }])
            && plan.writes.is_empty()
            && plan.exclude == Some(ExcludeAction::Ensure),
        "{plan:?}"
    );
}

#[test]
fn a_missing_exclude_rule_is_ensured_alongside_the_rest() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let mut project = with_agent_files(&set, &[("AGENTS.md", None)]);
    project.exclude = Exclude::Missing;
    let plan = plan(&FakeSource::new(), &project, false);
    assert!(
        matches!(plan.repos.as_slice(), [RepoAction::Materialise { .. }])
            && plan.writes.len() == 1
            && plan.exclude == Some(ExcludeAction::Ensure),
        "{plan:?}"
    );
}

#[test]
fn no_git_repo_is_a_note_and_not_drift() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let mut project = project(&set);
    project.exclude = Exclude::NoGit;
    let plan = plan(&in_sync_source(), &project, false);
    assert!(empty_but_for(&plan, ExcludeAction::NoGit), "{plan:?}");
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
    for action in &first.repos {
        match action {
            RepoAction::Remove { id } => source.remove(id).unwrap(),
            RepoAction::Materialise { repo, pin, .. } => {
                source.materialise(*repo, pin, Default::default()).unwrap()
            }
            RepoAction::Replace { .. } => panic!("unexpected {action:?}"),
        }
    }
    for write in &first.writes {
        project
            .agent_files
            .iter_mut()
            .find(|f| f.path == write.path)
            .unwrap()
            .text = Some(write.text.clone());
    }
    assert!(empty(&plan(&source, &project, false)));
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
        assert_eq!(format!("{plain:?}"), format!("{forced:?}"));
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
    assert_eq!(plan.refusals.len(), 1);
}

#[test]
fn a_dangling_non_active_name_is_left_for_doctor() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = in_sync_source();
    source.seed("old", Observed::Dangling);
    let project = project(&set);
    assert!(empty(&plan_with(&source, &["old"], &project, false)));
}

#[test]
fn every_active_repo_and_each_other_listed_name_is_inspected_once() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let mut asked = Vec::new();
    // the listing names an active repo too, and a name that is not active
    let listing = ["a".to_string(), "old".to_string()];
    Checkouts::observe(&set, &listing, |name| {
        asked.push(name.to_string());
        Ok::<_, ()>(Observed::Absent)
    })
    .unwrap();
    assert_eq!(asked, ["a", "old"]);
}

#[test]
fn inspect_failures_are_all_collected() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let listing = ["old".to_string()];
    let errors = Checkouts::observe(&set, &listing, |name| {
        Err::<Observed, _>(format!("cannot inspect {name}"))
    })
    .unwrap_err();
    assert_eq!(errors, ["cannot inspect a", "cannot inspect old"]);
}

#[test]
fn only_actions_on_a_checkout_the_block_lists_gate_the_agent_file_writes() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("old", at(OTHER_SHA, &[], &[]));
    let plan = plan_with(&source, &["old"], &project(&set), false);
    let gates: Vec<(&str, bool)> = plan
        .repos
        .iter()
        .map(|a| match a {
            RepoAction::Remove { id } => (id.as_str(), a.gates_writes()),
            RepoAction::Materialise { repo, .. } | RepoAction::Replace { repo, .. } => {
                (repo.id, a.gates_writes())
            }
        })
        .collect();
    // `old` is only removed, `a` is materialised and listed in the block
    assert_eq!(gates, [("old", false), ("a", true)]);
}

#[test]
fn a_replace_is_one_action_so_a_dangling_checkout_has_no_loose_remove() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("a", Observed::Dangling);
    let plan = plan(&source, &project(&set), false);
    assert!(matches!(
        plan.repos.as_slice(),
        [RepoAction::Replace { .. }]
    ));
    assert!(plan.repos[0].gates_writes());
}

const MARKED: &str = "# Notes\n\n<!-- BEGIN:refs -->\nold\n<!-- END:refs -->\n\nafter\n";

#[test]
fn with_no_active_repo_the_block_is_removed_and_the_rest_kept() {
    let config = parse("[settings]\n").unwrap();
    let set = active(&config);
    let project = with_agent_files(
        &set,
        &[
            ("AGENTS.md", Some(MARKED)),
            ("CLAUDE.md", Some("no markers\n")),
            ("OTHER.md", None),
        ],
    );
    let empty_lock = Lock::new(vec![]);
    let observed = Checkouts::observe(&set, &[], |_| Ok::<_, ()>(Observed::Absent)).unwrap();
    let plan = plan_checkouts(&set, &empty_lock, &observed, &project, false).unwrap();
    let writes: Vec<(&str, &str)> = plan
        .writes
        .iter()
        .map(|w| (w.path.as_str(), w.text.as_str()))
        .collect();
    assert_eq!(writes, [("AGENTS.md", "# Notes\n\n\n\nafter\n")]);
    assert!(plan.is_drift());
}

#[test]
fn a_failed_action_on_a_listed_checkout_holds_back_the_writes() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let source = FakeSource::new();
    source.seed("old", at(OTHER_SHA, &[], &[]));
    // repos: [Remove old, Materialise a]
    let plan = plan_with(&source, &["old"], &project(&set), false);
    assert!(!plan.holds_back_writes(&[]));
    assert!(!plan.holds_back_writes(&[0]));
    assert!(plan.holds_back_writes(&[1]));
}

#[test]
fn outcomes_follow_from_the_plan() {
    let config = parse(CONFIG).unwrap();
    let set = active(&config);
    let in_sync = plan(&in_sync_source(), &project(&set), false);
    assert_eq!(check_outcome(Some(&in_sync), false), Outcome::InSync);
    assert_eq!(check_outcome(Some(&in_sync), true), Outcome::OutOfDate);
    assert_eq!(check_outcome(None, false), Outcome::OutOfDate);

    let absent = plan(&FakeSource::new(), &project(&set), false);
    assert_eq!(check_outcome(Some(&absent), false), Outcome::OutOfDate);

    let source = FakeSource::new();
    source.seed("a", Observed::Foreign);
    let refused = plan(&source, &project(&set), false);
    assert_eq!(check_outcome(Some(&refused), true), Outcome::Refused);

    assert_eq!(Plan::applied_outcome(false, false), Outcome::InSync);
    assert_eq!(Plan::applied_outcome(true, false), Outcome::Refused);
    assert_eq!(Plan::applied_outcome(true, true), Outcome::Failed);
    assert!(refused.is_refused() && !absent.is_refused());
}
