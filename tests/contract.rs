//! One contract for `Source`, written once and run against the fake and against git. The git
//! adapter is the reference: a row the fake fails is a bug in the fake. Each row seeds one
//! Observed state, makes one call and asserts both the result and the state left behind.

mod common;

use std::fs;
use std::path::PathBuf;

use common::git;
use refs_cli::config::{Repo, RepoRef, parse};
use refs_cli::source::fake::FakeSource;
use refs_cli::source::git::GitSource;
use refs_cli::source::{MaterialiseOpts, Observed, Pin, Source};
use tempfile::TempDir;

/// The one Repo every row is about, and the paths the config asks for; `OTHER_PATHS` is what a
/// Checkout made under another config holds. Both exist in the remote of `GitHarness`.
const ID: &str = "r";
const PATHS: [&str; 1] = ["docs"];
const OTHER_PATHS: [&str; 1] = ["src"];
const STRAY: &str = "stray.txt";
/// The fake's remote: it is never fetched, so any URL will do.
const FAKE_URL: &str = "https://example.com/r";

/// A world to seed and a `Source` over it. Test support only: `Source` has no way to seed.
trait Harness {
    /// Two different commits a Checkout can be at.
    fn pins(&self) -> [Pin; 2];
    /// The remote the pins point at.
    fn url(&self) -> String;
    /// Put `ID` into `state`. An `At` must use one of `pins()`.
    fn seed(&self, state: Observed);
    fn source(&self) -> &dyn Source;
    /// Tag the remote's first commit with a lightweight tag for each of `lightweight` and an
    /// annotated one for each of `annotated`.
    fn tag(&self, lightweight: &[&str], annotated: &[&str]);
    /// The Repo `ID` on `url()`, as a user would write it, with the paths `PATHS`.
    fn repo(&self) -> Repo {
        repo_with(&self.url(), &PATHS)
    }
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
    fn url(&self) -> String {
        FAKE_URL.into()
    }
    fn pins(&self) -> [Pin; 2] {
        ["a", "b"].map(|c| Pin::git(FAKE_URL, "main", &c.repeat(40), None))
    }
    fn seed(&self, state: Observed) {
        self.source.seed(ID, state);
    }
    fn source(&self) -> &dyn Source {
        &self.source
    }
    fn tag(&self, lightweight: &[&str], annotated: &[&str]) {
        let names = lightweight.iter().chain(annotated);
        self.source.set_tags(FAKE_URL, names.copied());
    }
}

/// A local remote with two commits, a Cache and a references directory.
///
/// Seeding an `At` goes through `materialise`, the code the rows test: the Cache layout and the
/// Record beside git's worktree are private to the adapter, so seeding without it is not cheap.
/// What keeps the `inspect` rows honest is that the expected value is the test's own literal
/// while `inspect` reads git's HEAD and status; the fake's rows are a round trip of its seed.
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
        fs::create_dir_all(remote.join("src")).unwrap();
        git(&remote, &["init", "-q", "-b", "main"]);
        git(&remote, &["config", "uploadpack.allowFilter", "true"]);
        let mut shas = vec![];
        for text in ["one", "two"] {
            fs::write(remote.join("docs/a.md"), text).unwrap();
            fs::write(remote.join("src/b.md"), text).unwrap();
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

    fn checkout(&self) -> PathBuf {
        self.dir.path().join("refs").join(ID)
    }

    fn materialise(&self, pin: &Pin, paths: &[String]) {
        let repo = repo_with(&self.url(), paths);
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
    fn url(&self) -> String {
        common::file_url(&self.remote)
    }
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
                self.materialise(&self.pins()[0], &PATHS.map(String::from));
                fs::remove_dir_all(self.dir.path().join("cache")).unwrap();
            }
            Observed::At {
                pin,
                paths,
                dirty_files,
            } => {
                self.materialise(&pin, &paths);
                for file in dirty_files {
                    fs::write(self.checkout().join(file), "stray").unwrap();
                }
            }
        }
    }
    fn source(&self) -> &dyn Source {
        &self.source
    }
    fn tag(&self, lightweight: &[&str], annotated: &[&str]) {
        for name in lightweight {
            git(&self.remote, &["tag", name, &self.shas[0]]);
        }
        for name in annotated {
            git(
                &self.remote,
                &["tag", "-a", "-m", name, name, &self.shas[0]],
            );
        }
    }
}

/// The Repo `ID` on `url` with `paths`, as a user would write it.
fn repo_with(url: &str, paths: &[impl AsRef<str>]) -> Repo {
    let paths: Vec<String> = paths
        .iter()
        .map(|p| format!("\"{}\"", p.as_ref()))
        .collect();
    let text = format!(
        "[repos.{ID}]\nurl = \"{url}\"\nref = \"main\"\npaths = [{}]\n",
        paths.join(", ")
    );
    parse(&text).unwrap().repos.into_values().next().unwrap()
}

fn at(pin: Pin, paths: &[&str], dirty: bool) -> Observed {
    Observed::At {
        pin,
        paths: paths.iter().map(|p| p.to_string()).collect(),
        dirty_files: if dirty { vec![STRAY.into()] } else { vec![] },
    }
}

/// Every state a Checkout can be observed in, by name. `At` is at the first commit, with `PATHS`,
/// unless the name says otherwise.
fn states(h: &impl Harness) -> Vec<(&'static str, Observed)> {
    let [first, second] = h.pins();
    vec![
        ("absent", Observed::Absent),
        ("dangling", Observed::Dangling),
        ("foreign", Observed::Foreign),
        ("at, same commit", at(first.clone(), &PATHS, false)),
        ("at, other commit", at(second, &PATHS, false)),
        ("at, other paths", at(first.clone(), &OTHER_PATHS, false)),
        ("at, dirty", at(first, &PATHS, true)),
    ]
}

fn contract<H: Harness>(new: impl Fn() -> H) {
    inspect_rows(&new);
    remove_rows(&new);
    materialise_rows(&new);
    tags_rows(&new);
}

/// `tags` lists the names of the remote's tags, sorted, with an annotated tag once and by its
/// own name (not the `^{}` of its commit), and nothing for a remote with no tags.
fn tags_rows<H: Harness>(new: &impl Fn() -> H) {
    let h = new();
    assert_eq!(
        h.source().tags(&h.url()).unwrap(),
        Vec::<String>::new(),
        "tags: none"
    );
    h.tag(&["v1.0.0", "a-1.0.0"], &["v2.0.0"]);
    assert_eq!(
        h.source().tags(&h.url()).unwrap(),
        ["a-1.0.0", "v1.0.0", "v2.0.0"],
        "tags: lightweight and annotated"
    );
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

/// `materialise` makes the first commit's Checkout from Absent, and moves one of ours there
/// from another commit or from a dirty one (a move is a fresh Checkout; whether edits matter is
/// for `plan`). It refuses a Foreign directory and a Dangling Checkout, leaving them as they were.
fn materialise_rows<H: Harness>(new: &impl Fn() -> H) {
    for (name, state) in states(&new()) {
        let h = new();
        let [first, _] = h.pins();
        h.seed(state.clone());
        let repo = h.repo();
        let result = h.source().materialise(
            RepoRef {
                id: ID,
                repo: &repo,
            },
            &first,
            MaterialiseOpts::default(),
        );
        let (ok, left) = match state {
            Observed::Foreign | Observed::Dangling => (false, state),
            _ => (true, at(first, &PATHS, false)),
        };
        assert_eq!(result.is_ok(), ok, "materialise: {name}: {result:?}");
        assert_eq!(
            h.source().inspect(ID).unwrap(),
            left,
            "materialise, after: {name}"
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
