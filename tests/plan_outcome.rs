use refs_cli::diagnostic::Refusal;
use refs_cli::plan::{Outcome, Plan, RepoAction, check_outcome, conclude};

fn reports(names: &[&str]) -> Vec<miette::Report> {
    names.iter().map(|n| miette::miette!("{n}")).collect()
}

fn messages(diagnostics: &[miette::Report]) -> Vec<String> {
    diagnostics.iter().map(|d| d.to_string()).collect()
}

#[test]
fn without_stage_one_failures_the_outcome_is_stage_twos() {
    for stage_two in [
        Outcome::InSync,
        Outcome::OutOfDate,
        Outcome::Refused,
        Outcome::Failed,
    ] {
        assert_eq!(conclude(vec![], stage_two, vec![]).outcome, stage_two);
    }
}

#[test]
fn a_stage_one_failure_beats_every_stage_two_result() {
    for stage_two in [
        Outcome::InSync,
        Outcome::OutOfDate,
        Outcome::Refused,
        Outcome::Failed,
    ] {
        let concluded = conclude(reports(&["one"]), stage_two, vec![]);
        assert_eq!(concluded.outcome, Outcome::Failed, "{stage_two:?}");
    }
}

#[test]
fn stage_one_errors_come_before_stage_two_diagnostics() {
    let concluded = conclude(
        reports(&["lock a", "lock b"]),
        Outcome::Refused,
        reports(&["dirty c"]),
    );
    assert_eq!(
        messages(&concluded.diagnostics),
        ["lock a", "lock b", "dirty c"]
    );
}

fn plan(refusals: usize, repos: usize) -> Plan<'static> {
    Plan {
        repos: (0..repos)
            .map(|i| RepoAction::Remove {
                id: format!("r{i}"),
            })
            .collect(),
        writes: vec![],
        exclude: None,
        refusals: (0..refusals)
            .map(|_| Refusal::ForeignDir { id: "x".into() })
            .collect(),
    }
}

#[test]
fn check_with_no_plan_is_out_of_date() {
    assert_eq!(check_outcome(None, true), Outcome::OutOfDate);
}

#[test]
fn check_a_refusal_outranks_drift() {
    assert_eq!(check_outcome(Some(&plan(1, 1)), true), Outcome::Refused);
}

#[test]
fn check_drift_in_the_plan_or_the_lock_is_out_of_date() {
    assert_eq!(check_outcome(Some(&plan(0, 1)), false), Outcome::OutOfDate);
    assert_eq!(check_outcome(Some(&plan(0, 0)), true), Outcome::OutOfDate);
}

#[test]
fn check_nothing_to_do_is_in_sync() {
    assert_eq!(check_outcome(Some(&plan(0, 0)), false), Outcome::InSync);
}

#[test]
fn applying_a_plan_is_refused_if_it_has_refusals_else_in_sync() {
    assert_eq!(plan(1, 0).applied_outcome(), Outcome::Refused);
    assert_eq!(plan(0, 2).applied_outcome(), Outcome::InSync);
}
