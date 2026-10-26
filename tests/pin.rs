use refs_cli::source::Pin;

const SHA: &str = "1a2b3c4d5e6f0000000000000000000000000000";

#[test]
fn a_pin_shows_a_short_id_and_the_branch_for_head() {
    let head = Pin::new("u", "HEAD", SHA, Some("main"));
    assert_eq!(head.short_id(), "1a2b3c4");
    assert_eq!(head.display_ref(), "main");
}

#[test]
fn a_pin_shows_its_ref_when_there_is_no_branch() {
    assert_eq!(Pin::new("u", "next", SHA, None).display_ref(), "next");
    // A detached remote HEAD records no branch.
    assert_eq!(Pin::new("u", "HEAD", SHA, None).display_ref(), "HEAD");
}
