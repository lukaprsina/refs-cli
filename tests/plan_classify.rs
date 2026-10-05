use refs_cli::config::{Repo, parse};
use refs_cli::plan::{Cause, CheckoutState, classify};
use refs_cli::source::{Observed, Pin};

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

fn at(sha: &str, paths: &[&str], dirty: &[&str]) -> Observed {
    Observed::At {
        pin: pin(sha),
        paths: paths.iter().map(|p| p.to_string()).collect(),
        dirty_files: dirty.iter().map(|p| p.to_string()).collect(),
    }
}

fn stale(cause: Cause, dirty: &[&str]) -> CheckoutState {
    CheckoutState::Stale {
        cause,
        dirty_files: dirty.iter().map(|p| p.to_string()).collect(),
    }
}

#[test]
fn classifies_what_is_on_disk_against_the_lock() {
    use CheckoutState::{Absent, Dangling, Foreign, InSync};
    let config = parse(CONFIG).unwrap();
    let repo: &Repo = config.repos.values().next().unwrap();
    let cases = [
        ("absent", Observed::Absent, Absent),
        ("dangling", Observed::Dangling, Dangling),
        ("foreign", Observed::Foreign, Foreign),
        ("matching", at(SHA, &["docs", "src"], &[]), InSync),
        (
            "matching but dirty",
            at(SHA, &["docs", "src"], &["notes.md"]),
            InSync,
        ),
        (
            "paths compared as a set",
            at(SHA, &["src", "docs", "src"], &[]),
            InSync,
        ),
        (
            "wrong commit",
            at(OTHER_SHA, &["docs", "src"], &[]),
            stale(Cause::Commit, &[]),
        ),
        (
            "wrong commit, dirty",
            at(OTHER_SHA, &["docs", "src"], &["a.md", "b.md"]),
            stale(Cause::Commit, &["a.md", "b.md"]),
        ),
        (
            "wrong paths",
            at(SHA, &["docs"], &[]),
            stale(Cause::Paths, &[]),
        ),
        (
            "wrong paths, dirty",
            at(SHA, &["docs"], &["a.md"]),
            stale(Cause::Paths, &["a.md"]),
        ),
        (
            "wrong commit and wrong paths is a wrong commit",
            at(OTHER_SHA, &["docs"], &[]),
            stale(Cause::Commit, &[]),
        ),
    ];
    for (name, observed, expected) in cases {
        assert_eq!(classify(&observed, repo, &pin(SHA)), expected, "{name}");
    }
}
