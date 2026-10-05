//! One contract for `Source`, written once and run against the fake and against git. The git
//! adapter is the reference: a row the fake fails is a bug in the fake. Each row seeds one
//! Observed state, makes one call and asserts both the result and the state left behind.

mod common;

use std::fs;
use std::path::PathBuf;

use common::git;
use refs_cli::config::{RepoRef, parse};
use refs_cli::source::fake::FakeSource;
use refs_cli::source::git::GitSource;
use refs_cli::source::{MaterialiseOpts, Observed, Pin, Source};
use tempfile::TempDir;

const ID: &str = "r";
const PATHS: [&str; 1] = ["docs"];
const STRAY: &str = "stray.txt";

/// A world to seed and a `Source` over it. Test support only: `Source` has no way to seed.
trait Harness {
    /// Two different commits a Checkout can be at.
    fn pins(&self) -> [Pin; 2];
    /// Put `ID` into `state`. An `At` must use one of `pins()`.
    fn seed(&self, state: Observed);
    fn source(&self) -> &dyn Source;
}

struct FakeHarness {
    source: FakeSource,
}

impl FakeHarness {
    fn new() -> Self {
        FakeHarness {
            source: FakeSource::new(),
        }
    }
}

impl Harness for FakeHarness {
    fn pins(&self) -> [Pin; 2] {
        ["a", "b"].map(|c| Pin::git("https://example.com/r", "main", &c.repeat(40), None))
    }
    fn seed(&self, state: Observed) {
        self.source.seed(ID, state);
    }
    fn source(&self) -> &dyn Source {
        &self.source
    }
}

/// A local remote with two commits, a Cache and a references directory.
struct GitHarness {
    dir: TempDir,
    remote: PathBuf,
    shas: [String; 2],
    source: GitSource,
}

impl GitHarness {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let remote = dir.path().join("remote");
        fs::create_dir_all(remote.join("docs")).unwrap();
        git(&remote, &["init", "-q", "-b", "main"]);
        git(&remote, &["config", "uploadpack.allowFilter", "true"]);
        let mut shas = vec![];
        for text in ["one", "two"] {
            fs::write(remote.join("docs/a.md"), text).unwrap();
            git(&remote, &["add", "."]);
            git(&remote, &["commit", "-q", "-m", text]);
            shas.push(git(&remote, &["rev-parse", "HEAD"]));
        }
        let source = GitSource::new(dir.path().join("cache"), dir.path().join("refs"));
        GitHarness {
            remote,
            shas: [shas.remove(0), shas.remove(0)],
            source,
            dir,
        }
    }

    fn url(&self) -> String {
        common::file_url(&self.remote)
    }

    fn checkout(&self) -> PathBuf {
        self.dir.path().join("refs").join(ID)
    }

    fn materialise(&self, pin: &Pin) {
        let text = format!(
            "[repos.{ID}]\nurl = \"{}\"\nref = \"main\"\npaths = [\"docs\"]\n",
            self.url()
        );
        let repo = parse(&text).unwrap().repos.into_values().next().unwrap();
        self.source
            .materialise(
                RepoRef {
                    id: ID,
                    repo: &repo,
                },
                pin,
                MaterialiseOpts::default(),
            )
            .unwrap();
    }
}

impl Harness for GitHarness {
    fn pins(&self) -> [Pin; 2] {
        self.shas
            .clone()
            .map(|sha| Pin::git(&self.url(), "main", &sha, None))
    }
    fn seed(&self, state: Observed) {
        match state {
            Observed::Absent => {}
            Observed::Foreign => {
                fs::create_dir_all(self.checkout()).unwrap();
                fs::write(self.checkout().join("mine.txt"), "mine").unwrap();
            }
            Observed::Dangling => {
                self.materialise(&self.pins()[0]);
                fs::remove_dir_all(self.dir.path().join("cache")).unwrap();
            }
            Observed::At {
                pin, dirty_files, ..
            } => {
                self.materialise(&pin);
                for file in dirty_files {
                    fs::write(self.checkout().join(file), "stray").unwrap();
                }
            }
        }
    }
    fn source(&self) -> &dyn Source {
        &self.source
    }
}

fn at(pin: Pin, dirty: bool) -> Observed {
    Observed::At {
        pin,
        paths: PATHS.map(String::from).to_vec(),
        dirty_files: if dirty { vec![STRAY.into()] } else { vec![] },
    }
}

/// Every state a Checkout can be observed in, by name. `At` is at the first commit unless the
/// name says otherwise.
fn states(h: &impl Harness) -> Vec<(&'static str, Observed)> {
    let [first, second] = h.pins();
    vec![
        ("absent", Observed::Absent),
        ("dangling", Observed::Dangling),
        ("foreign", Observed::Foreign),
        ("at, same commit", at(first.clone(), false)),
        ("at, other commit", at(second, false)),
        ("at, dirty", at(first, true)),
    ]
}

fn contract<H: Harness>(new: impl Fn() -> H) {
    inspect_rows(&new);
    remove_rows(&new);
}

/// `inspect` reports what was seeded, and looking changes nothing.
fn inspect_rows<H: Harness>(new: &impl Fn() -> H) {
    for (name, state) in states(&new()) {
        let h = new();
        h.seed(state.clone());
        assert_eq!(h.source().inspect(ID).unwrap(), state, "inspect: {name}");
        assert_eq!(
            h.source().inspect(ID).unwrap(),
            state,
            "inspect twice: {name}"
        );
    }
}

/// `remove` takes every Checkout of ours (a dirty one too: whether edits matter is for `plan`),
/// is a no-op on Absent, and refuses a Foreign directory, leaving it in place.
fn remove_rows<H: Harness>(new: &impl Fn() -> H) {
    for (name, state) in states(&new()) {
        let h = new();
        h.seed(state.clone());
        let result = h.source().remove(ID);
        let (ok, left) = match state {
            Observed::Foreign => (false, Observed::Foreign),
            _ => (true, Observed::Absent),
        };
        assert_eq!(result.is_ok(), ok, "remove: {name}: {result:?}");
        assert_eq!(
            h.source().inspect(ID).unwrap(),
            left,
            "remove, after: {name}"
        );
    }
}

#[test]
fn the_fake_meets_the_contract() {
    contract(FakeHarness::new);
}

#[test]
fn git_meets_the_contract() {
    contract(GitHarness::new);
}
