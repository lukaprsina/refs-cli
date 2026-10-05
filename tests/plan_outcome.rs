use refs_cli::plan::{Outcome, conclude};

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
