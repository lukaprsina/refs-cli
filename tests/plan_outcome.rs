use refs_cli::diagnostic::Refusal;
use refs_cli::plan::{Outcome, Plan, RepoAction, check_outcome};

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
