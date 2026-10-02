use miette::Diagnostic;
use refs_cli::config::{Repo, RepoRef, parse};
use refs_cli::source::fake::{FakeSource, Method};
use refs_cli::source::{MaterialiseOpts, Observed, Pin, Source};

fn at<'a>(id: &'a str, repo: &'a Repo) -> RepoRef<'a> {
    RepoRef { id, repo }
}

fn pin() -> Pin {
    Pin::git(
        "https://example.com/a",
        "HEAD",
        &"a".repeat(40),
        Some("main"),
    )
}

#[test]
fn an_unseeded_repo_is_absent() {
    let source = FakeSource::new();
    assert_eq!(source.inspect("solid").unwrap(), Observed::Absent);
}

#[test]
fn the_fake_reports_every_observed_state() {
    let source = FakeSource::new();
    let at = Observed::At {
        pin: pin(),
        paths: vec!["packages".into()],
        dirty_files: vec!["packages/x.txt".into()],
    };
    source.seed("a", Observed::Dangling);
    source.seed("b", Observed::Foreign);
    source.seed("c", at.clone());

    assert_eq!(source.inspect("a").unwrap(), Observed::Dangling);
    assert_eq!(source.inspect("b").unwrap(), Observed::Foreign);
    assert_eq!(source.inspect("c").unwrap(), at);
}

fn repo() -> Repo {
    let text = r#"
[repos.solid]
url = "https://github.com/solidjs/solid"
ref = "next"
paths = ["packages"]
"#;
    parse(text).unwrap().repos.into_values().next().unwrap()
}

#[test]
fn materialise_creates_the_checkout_and_remove_deletes_it() {
    let source = FakeSource::new();
    let repo = repo();

    let pin = source.resolve(at("solid", &repo)).unwrap();
    source
        .materialise(at("solid", &repo), &pin, MaterialiseOpts::default())
        .unwrap();
    assert_eq!(
        source.inspect("solid").unwrap(),
        Observed::At {
            pin,
            paths: vec!["packages".into()],
            dirty_files: vec![],
        }
    );

    source.remove("solid").unwrap();
    assert_eq!(source.inspect("solid").unwrap(), Observed::Absent);
}

#[test]
fn an_injected_failure_hits_only_that_repo_and_method() {
    let source = FakeSource::new();
    let repo = repo();
    let opts = MaterialiseOpts::default();
    let pin = source.resolve(at("ok", &repo)).unwrap();
    source.fail("bad", Method::Verify, "boom");

    let err = source.verify(at("bad", &repo), &pin).unwrap_err();
    assert_eq!(err.code().unwrap().to_string(), "refs::git::failed");
    assert!(source.verify(at("ok", &repo), &pin).is_ok());
    // Other methods on the failing repo still work.
    assert!(source.resolve(at("bad", &repo)).is_ok());
    assert!(source.materialise(at("bad", &repo), &pin, opts).is_ok());
}

#[test]
fn every_method_can_fail() {
    let repo = repo();
    let opts = MaterialiseOpts::default();
    let source = FakeSource::new();
    let pin = source.resolve(at("a", &repo)).unwrap();
    for method in [
        Method::Resolve,
        Method::Verify,
        Method::Materialise,
        Method::Remove,
    ] {
        source.fail("a", method, "boom");
    }
    assert!(source.resolve(at("a", &repo)).is_err());
    assert!(source.verify(at("a", &repo), &pin).is_err());
    assert!(source.materialise(at("a", &repo), &pin, opts).is_err());
    assert!(source.remove("a").is_err());
}

#[test]
fn a_failed_materialise_leaves_the_checkout_untouched() {
    let source = FakeSource::new();
    let repo = repo();
    let pin = source.resolve(at("a", &repo)).unwrap();
    source.fail("a", Method::Materialise, "boom");
    source.seed("a", Observed::Dangling);

    assert!(
        source
            .materialise(at("a", &repo), &pin, MaterialiseOpts::default())
            .is_err()
    );
    assert_eq!(source.inspect("a").unwrap(), Observed::Dangling);
}

#[test]
fn a_pin_shows_a_short_id_and_the_branch_for_head() {
    let sha = "1a2b3c4d5e6f".to_string() + &"0".repeat(28);
    let head = Pin::git("u", "HEAD", &sha, Some("main"));
    assert_eq!(head.short_id(), "1a2b3c4");
    assert_eq!(head.display_ref(), "main");
}

#[test]
fn a_pin_shows_its_ref_when_there_is_no_branch() {
    let sha = "1a2b3c4d5e6f".to_string() + &"0".repeat(28);
    assert_eq!(Pin::git("u", "next", &sha, None).display_ref(), "next");
    // A detached remote HEAD records no branch.
    assert_eq!(Pin::git("u", "HEAD", &sha, None).display_ref(), "HEAD");
}
