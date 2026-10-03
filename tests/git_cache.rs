//! `GitSource::verify` and `materialise` against real git: a local `file://` remote that
//! allows filters (spec §7.1), a Cache in a temporary directory, Checkouts beside it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use miette::Diagnostic;
use refs_cli::config::{Repo, RepoRef, parse};
use refs_cli::diagnostic::SourceError;
use refs_cli::source::git::GitSource;
use refs_cli::source::git::remote::cache_dir_name;
use refs_cli::source::{MaterialiseOpts, Pin, Source, VerifyOpts};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn code(e: impl Diagnostic) -> String {
    e.code().expect("a code").to_string()
}

const ONLINE: bool = false;
const OFFLINE: bool = true;

/// A remote with one commit, tagged `v1` (annotated):
///
/// ```text
/// README.md  LICENSE            root files, checked out by cone mode
/// docs/other.md                 a file in an ancestor directory of `docs/guide`
/// docs/guide/a.md  docs/guide/sub/b.md
/// docs/api/c.md  src/lib.rs
/// ```
struct Remote {
    dir: TempDir,
    sha: String,
}

impl Remote {
    fn new() -> Remote {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "uploadpack.allowFilter", "true"]);
        for (file, text) in [
            ("README.md", "readme"),
            ("LICENSE", "license"),
            ("docs/other.md", "other"),
            ("docs/guide/a.md", "a"),
            ("docs/guide/sub/b.md", "b"),
            ("docs/api/c.md", "c"),
            ("src/lib.rs", "lib"),
        ] {
            let path = p.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "one"]);
        git(p, &["tag", "-a", "-m", "release", "v1"]);
        let sha = git(p, &["rev-parse", "HEAD"]);
        Remote { dir, sha }
    }

    fn url(&self) -> String {
        format!("file://{}", self.dir.path().display())
    }

    fn pin(&self) -> Pin {
        Pin::git(&self.url(), "main", &self.sha, None)
    }

    /// The ids of the blobs at `files`, as the remote has them.
    fn blobs(&self, files: &[&str]) -> BTreeSet<String> {
        files
            .iter()
            .map(|f| {
                git(
                    self.dir.path(),
                    &["rev-parse", &format!("{}:{f}", self.sha)],
                )
            })
            .collect()
    }

    fn repo(&self, paths: &[&str], start: &[&str]) -> Repo {
        let list = |items: &[&str]| {
            let quoted: Vec<String> = items.iter().map(|i| format!("\"{i}\"")).collect();
            format!("[{}]", quoted.join(", "))
        };
        let text = format!(
            "[repos.r]\nurl = \"{}\"\nref = \"main\"\npaths = {}\nstart = {}\n",
            self.url(),
            list(paths),
            list(start)
        );
        parse(&text).unwrap().repos.into_values().next().unwrap()
    }
}

/// A Cache and a references directory, in a temporary directory of their own.
struct Env {
    dir: TempDir,
}

impl Env {
    fn new() -> Env {
        Env {
            dir: TempDir::new().unwrap(),
        }
    }

    fn cache(&self) -> PathBuf {
        self.dir.path().join("cache")
    }

    fn checkouts(&self) -> PathBuf {
        self.dir.path().join("project/.references")
    }

    fn source(&self) -> GitSource {
        GitSource::new(self.cache(), self.checkouts())
    }

    /// The bare repository the Cache keeps for `remote`.
    fn cache_repo(&self, remote: &Remote) -> PathBuf {
        self.cache().join("git").join(cache_dir_name(&remote.url()))
    }

    /// The blobs the Cache repository holds.
    fn cached_blobs(&self, remote: &Remote) -> BTreeSet<String> {
        let out = git(
            &self.cache_repo(remote),
            &[
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objecttype) %(objectname)",
            ],
        );
        out.lines()
            .filter_map(|l| l.strip_prefix("blob "))
            .map(String::from)
            .collect()
    }

    /// The files of a Checkout, relative to it, `.git` excluded.
    fn files(&self, id: &str) -> BTreeSet<String> {
        let root = self.checkouts().join(id);
        let mut found = BTreeSet::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path.strip_prefix(&root).unwrap();
                    found.insert(rel.to_str().unwrap().to_string());
                }
            }
        }
        found
    }
}

fn at<'a>(id: &'a str, repo: &'a Repo) -> RepoRef<'a> {
    RepoRef { id, repo }
}

fn verify(source: &GitSource, remote: &Remote, repo: &Repo, offline: bool) -> Result<(), String> {
    source
        .verify(at("r", repo), &remote.pin(), VerifyOpts { offline })
        .map_err(code)
}

fn materialise(
    source: &GitSource,
    id: &str,
    remote: &Remote,
    repo: &Repo,
    offline: bool,
) -> Result<(), String> {
    source
        .materialise(at(id, repo), &remote.pin(), MaterialiseOpts { offline })
        .map_err(code)
}

fn names(files: &[&str]) -> BTreeSet<String> {
    files.iter().map(|f| f.to_string()).collect()
}

mod verify {
    use super::*;

    #[test]
    fn existing_paths_and_start_files_pass() {
        let (remote, env) = (Remote::new(), Env::new());
        let repo = remote.repo(&["docs/guide", "src"], &["docs/guide/a.md", "README.md"]);
        assert_eq!(verify(&env.source(), &remote, &repo, ONLINE), Ok(()));
    }

    #[test]
    fn a_repo_without_paths_passes() {
        let (remote, env) = (Remote::new(), Env::new());
        let repo = remote.repo(&[], &[]);
        assert_eq!(verify(&env.source(), &remote, &repo, ONLINE), Ok(()));
    }

    #[test]
    fn the_cache_is_a_partial_clone_with_no_fetch_refspec() {
        let (remote, env) = (Remote::new(), Env::new());
        verify(&env.source(), &remote, &remote.repo(&["docs"], &[]), ONLINE).unwrap();

        let cache = env.cache_repo(&remote);
        assert_eq!(
            git(&cache, &["config", "remote.origin.partialclonefilter"]),
            "blob:none"
        );
        let refspecs = Command::new("git")
            .current_dir(&cache)
            .args(["config", "--get-all", "remote.origin.fetch"])
            .output()
            .unwrap();
        assert!(refspecs.stdout.is_empty(), "a bare clone has no refspec");
        assert!(
            env.cached_blobs(&remote).is_empty(),
            "no blobs were fetched"
        );
    }

    #[test]
    fn the_url_is_kept_beside_the_cache_repo() {
        let (remote, env) = (Remote::new(), Env::new());
        verify(&env.source(), &remote, &remote.repo(&[], &[]), ONLINE).unwrap();

        let url_file = env
            .cache()
            .join("git")
            .join(format!("{}.url", cache_dir_name(&remote.url())));
        assert_eq!(fs::read_to_string(url_file).unwrap().trim(), remote.url());
    }

    #[test]
    fn a_missing_path_is_an_error() {
        let (remote, env) = (Remote::new(), Env::new());
        let repo = remote.repo(&["docs/nope"], &[]);
        let err = env
            .source()
            .verify(at("r", &repo), &remote.pin(), VerifyOpts::default())
            .unwrap_err();
        assert_eq!(err.code().unwrap().to_string(), "refs::git::path_missing");
        let message = err.to_string();
        assert!(
            message.contains("docs/nope") && message.contains('r'),
            "{message}"
        );
        assert!(message.contains(&remote.sha), "{message}");
    }

    #[test]
    fn a_path_that_is_a_file_is_an_error() {
        let (remote, env) = (Remote::new(), Env::new());
        let repo = remote.repo(&["README.md"], &[]);
        assert_eq!(
            verify(&env.source(), &remote, &repo, ONLINE),
            Err("refs::git::path_not_dir".into())
        );
    }

    #[test]
    fn a_missing_start_file_is_an_error() {
        let (remote, env) = (Remote::new(), Env::new());
        let repo = remote.repo(&["docs"], &["docs/MIGRATION.md"]);
        assert_eq!(
            verify(&env.source(), &remote, &repo, ONLINE),
            Err("refs::git::start_missing".into())
        );
    }

    #[test]
    fn a_tag_object_is_not_a_commit() {
        let (remote, env) = (Remote::new(), Env::new());
        let tag_object = git(remote.dir.path(), &["rev-parse", "v1"]);
        assert_ne!(tag_object, remote.sha, "the fixture needs a tag object");
        let pin = Pin::git(&remote.url(), "v1", &tag_object, None);
        let repo = remote.repo(&[], &[]);

        let err = env
            .source()
            .verify(at("r", &repo), &pin, VerifyOpts::default())
            .unwrap_err();
        assert_eq!(code(err), "refs::git::unpeeled_tag");
    }

    #[test]
    fn a_commit_id_that_could_be_an_option_never_reaches_git() {
        let (remote, env) = (Remote::new(), Env::new());
        let pin = Pin::git(&remote.url(), "main", "--upload-pack=touch /tmp/x", None);
        let repo = remote.repo(&[], &[]);
        let source = env.source();

        let err = source
            .verify(at("r", &repo), &pin, VerifyOpts::default())
            .unwrap_err();
        assert_eq!(code(err), "refs::git::unsafe_input");
        let err = source
            .materialise(at("r", &repo), &pin, MaterialiseOpts::default())
            .unwrap_err();
        assert_eq!(code(err), "refs::git::unsafe_input");
        assert!(!env.cache_repo(&remote).exists());
    }

    #[test]
    fn a_commit_the_remote_does_not_have_is_an_error() {
        let (remote, env) = (Remote::new(), Env::new());
        let pin = Pin::git(&remote.url(), "main", &"0123456789".repeat(4), None);
        let repo = remote.repo(&[], &[]);

        let err = env
            .source()
            .verify(at("r", &repo), &pin, VerifyOpts::default())
            .unwrap_err();
        assert_eq!(code(err), "refs::git::commit_unavailable");
    }

    #[test]
    fn offline_with_an_empty_cache_fails_instead_of_fetching() {
        let (remote, env) = (Remote::new(), Env::new());
        let repo = remote.repo(&[], &[]);
        assert_eq!(
            verify(&env.source(), &remote, &repo, OFFLINE),
            Err("refs::git::not_cached".into())
        );
        assert!(!env.cache_repo(&remote).exists(), "nothing was cloned");
    }

    #[test]
    fn offline_passes_from_what_an_earlier_run_cached() {
        let (remote, env) = (Remote::new(), Env::new());
        let repo = remote.repo(&["docs/guide"], &["docs/guide/a.md"]);
        verify(&env.source(), &remote, &repo, ONLINE).unwrap();

        assert_eq!(verify(&env.source(), &remote, &repo, OFFLINE), Ok(()));
    }
}

mod materialise {
    use super::*;

    #[test]
    fn the_checkout_has_the_root_files_the_ancestor_files_and_the_paths() {
        let (remote, env) = (Remote::new(), Env::new());
        materialise(
            &env.source(),
            "r",
            &remote,
            &remote.repo(&["docs/guide"], &[]),
            ONLINE,
        )
        .unwrap();

        assert_eq!(
            env.files("r"),
            names(&[
                "README.md",
                "LICENSE",
                "docs/other.md",
                "docs/guide/a.md",
                "docs/guide/sub/b.md"
            ])
        );
        let checkout = env.checkouts().join("r");
        assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), remote.sha);
        assert_eq!(
            git(&checkout, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "HEAD"
        );
    }

    #[test]
    fn a_repo_without_paths_is_checked_out_whole() {
        let (remote, env) = (Remote::new(), Env::new());
        materialise(&env.source(), "r", &remote, &remote.repo(&[], &[]), ONLINE).unwrap();

        assert_eq!(env.files("r").len(), 7);
    }

    #[test]
    fn the_prefetch_brings_exactly_the_blobs_the_checkout_reads() {
        let (remote, env) = (Remote::new(), Env::new());
        materialise(
            &env.source(),
            "r",
            &remote,
            &remote.repo(&["docs/guide"], &[]),
            ONLINE,
        )
        .unwrap();

        assert_eq!(
            env.cached_blobs(&remote),
            remote.blobs(&[
                "README.md",
                "LICENSE",
                "docs/other.md",
                "docs/guide/a.md",
                "docs/guide/sub/b.md"
            ])
        );
    }

    #[test]
    fn a_second_repo_of_the_same_url_fetches_only_what_is_new() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        materialise(
            &source,
            "a",
            &remote,
            &remote.repo(&["docs/guide"], &[]),
            ONLINE,
        )
        .unwrap();
        materialise(&source, "b", &remote, &remote.repo(&["src"], &[]), ONLINE).unwrap();

        assert_eq!(
            env.files("b"),
            names(&["README.md", "LICENSE", "src/lib.rs"])
        );
        assert_eq!(
            env.files("a"),
            names(&[
                "README.md",
                "LICENSE",
                "docs/other.md",
                "docs/guide/a.md",
                "docs/guide/sub/b.md"
            ]),
            "the other checkout keeps its own sparse patterns"
        );
    }

    #[test]
    fn offline_without_the_blobs_names_an_object_and_leaves_no_worktree() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        // Verifying caches the commit and its trees, but no blobs.
        verify(&source, &remote, &repo, ONLINE).unwrap();

        let err = source
            .materialise(
                at("r", &repo),
                &remote.pin(),
                MaterialiseOpts { offline: OFFLINE },
            )
            .unwrap_err();

        let SourceError::ObjectMissing { repo: named, oid } = err else {
            panic!("expected object_missing, got {err:?}");
        };
        assert_eq!(named, "r");
        let read_by_checkout = remote.blobs(&[
            "README.md",
            "LICENSE",
            "docs/other.md",
            "docs/guide/a.md",
            "docs/guide/sub/b.md",
        ]);
        assert!(
            read_by_checkout.contains(&oid),
            "{oid} is not a blob of the checkout"
        );
        assert!(!env.checkouts().join("r").exists());
        let worktrees = git(
            &env.cache_repo(&remote),
            &["worktree", "list", "--porcelain"],
        );
        assert_eq!(worktrees.matches("worktree ").count(), 1, "{worktrees}");
    }

    #[test]
    fn offline_succeeds_once_the_blobs_are_present() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "first", &remote, &repo, ONLINE).unwrap();

        materialise(&source, "second", &remote, &repo, OFFLINE).unwrap();

        assert_eq!(env.files("second"), env.files("first"));
    }

    #[test]
    fn a_checkout_deleted_by_hand_is_recreated() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        fs::remove_dir_all(env.checkouts().join("r")).unwrap();

        materialise(&source, "r", &remote, &repo, OFFLINE).unwrap();

        assert!(env.files("r").contains("docs/guide/a.md"));
    }

    #[test]
    fn concurrent_first_use_of_one_cache_is_serialised_by_its_lock() {
        let (remote, env) = (Remote::new(), Env::new());
        let repo_a = remote.repo(&["docs/guide"], &[]);
        let repo_b = remote.repo(&["src"], &[]);
        let (a, b) = (env.source(), env.source());

        let (result_a, result_b) = std::thread::scope(|scope| {
            let a = scope.spawn(|| materialise(&a, "a", &remote, &repo_a, ONLINE));
            let b = scope.spawn(|| materialise(&b, "b", &remote, &repo_b, ONLINE));
            (a.join().unwrap(), b.join().unwrap())
        });

        assert_eq!((result_a, result_b), (Ok(()), Ok(())));
        assert!(env.files("a").contains("docs/guide/a.md"));
        assert!(env.files("b").contains("src/lib.rs"));
    }
}

/// A real server, not `file://`: it honours the filter for real, so the clone is partial and
/// a checkout brings only the blobs it reads. Opt in with `REFS_NETWORK_TESTS=1`.
#[test]
fn a_real_remote_serves_a_blobless_clone() {
    if std::env::var_os("REFS_NETWORK_TESTS").is_none() {
        eprintln!("skipped: set REFS_NETWORK_TESTS=1 to run against github.com");
        return;
    }
    let env = Env::new();
    let text = "[repos.r]\nurl = \"https://github.com/octocat/Hello-World\"\n";
    let repo = parse(text).unwrap().repos.into_values().next().unwrap();
    let source = env.source();

    let pin = source.resolve(at("r", &repo)).unwrap();
    source
        .materialise(at("r", &repo), &pin, MaterialiseOpts::default())
        .unwrap();

    let url = "https://github.com/octocat/Hello-World";
    let cache = env.cache().join("git").join(cache_dir_name(url));
    assert_eq!(
        git(&cache, &["config", "remote.origin.partialclonefilter"]),
        "blob:none"
    );
    assert!(env.checkouts().join("r/README").exists());
}
