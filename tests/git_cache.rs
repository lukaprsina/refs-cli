//! `GitSource::verify` and `materialise` against real git: a local `file://` remote that
//! allows filters (spec §7.1), a Cache in a temporary directory, Checkouts beside it.

mod common;

use common::git;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use miette::Diagnostic;
use refs_cli::config::{Repo, RepoRef, parse};
use refs_cli::diagnostic::SourceError;
use refs_cli::source::git::GitSource;
use refs_cli::source::git::remote::cache_dir_name;
use refs_cli::source::{MaterialiseOpts, Observed, Pin, Source, VerifyOpts};
use tempfile::TempDir;

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
        common::file_url(self.dir.path())
    }

    /// A second commit: `docs/guide/a.md` and `src/lib.rs` change, `docs/guide/new.md` appears.
    fn advance(&mut self) {
        let p = self.dir.path();
        fs::write(p.join("docs/guide/a.md"), "a, again").unwrap();
        fs::write(p.join("docs/guide/new.md"), "new").unwrap();
        fs::write(p.join("src/lib.rs"), "lib, again").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "two"]);
        self.sha = git(p, &["rev-parse", "HEAD"]);
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
                    let parts: Vec<_> = rel.iter().map(|c| c.to_str().unwrap()).collect();
                    found.insert(parts.join("/"));
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
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_WORK_TREE")
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
    fn a_root_directory_as_start_is_an_error_only_when_paths_leave_it_out() {
        let (remote, env) = (Remote::new(), Env::new());
        let with_paths = remote.repo(&["docs"], &["src"]);
        assert_eq!(
            verify(&env.source(), &remote, &with_paths, ONLINE),
            Err("refs::git::start_not_file".into())
        );
        // no `paths`: the whole repo is checked out, directories included
        let whole = remote.repo(&[], &["src"]);
        assert_eq!(verify(&env.source(), &remote, &whole, ONLINE), Ok(()));
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

    /// `prefetch` pipes every blob id to `git cat-file --batch-check` and reads the answers;
    /// with enough blobs neither pipe can hold what git and refs each have to say, so a
    /// refs that writes all of stdin before reading stdout never finishes.
    #[test]
    fn a_repo_with_thousands_of_blobs_is_checked_out() {
        const FILES: usize = 4000;
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "uploadpack.allowFilter", "true"]);
        fs::create_dir_all(p.join("many")).unwrap();
        for i in 0..FILES {
            fs::write(p.join(format!("many/{i}.txt")), i.to_string()).unwrap();
        }
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "many"]);
        let sha = git(p, &["rev-parse", "HEAD"]);
        let remote = Remote { dir, sha };
        let env = Env::new();

        // Not a scoped thread: a deadlock must fail the test, not hang its join.
        let (tx, rx) = std::sync::mpsc::channel();
        let (source, repo, pin_remote) = (env.source(), remote.repo(&["many"], &[]), remote);
        std::thread::spawn(move || {
            let result = materialise(&source, "r", &pin_remote, &repo, ONLINE);
            let _ = tx.send(result);
        });
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("materialise finished: refs and git are not waiting on each other");
        assert_eq!(result, Ok(()));
        assert_eq!(env.files("r").len(), FILES);
    }
}

mod checkout {
    use super::*;

    fn clean_at(remote: &Remote, paths: &[&str]) -> Observed {
        Observed::At {
            pin: remote.pin(),
            paths: paths.iter().map(|p| p.to_string()).collect(),
            dirty_files: vec![],
        }
    }

    #[test]
    fn nothing_on_disk_is_absent() {
        let env = Env::new();
        let source = env.source();

        assert_eq!(source.inspect("r").unwrap(), Observed::Absent);
        assert_eq!(source.list().unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_fresh_checkout_is_observed_at_its_pin_and_listed() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        fs::create_dir_all(env.checkouts().join("stray")).unwrap();

        assert_eq!(
            source.inspect("r").unwrap(),
            clean_at(&remote, &["docs/guide"])
        );
        assert_eq!(source.list().unwrap(), ["r", "stray"]);
    }

    #[test]
    fn modified_and_untracked_files_are_dirty() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        let checkout = env.checkouts().join("r");
        fs::write(checkout.join("docs/guide/a.md"), "edited").unwrap();
        fs::write(checkout.join("notes.txt"), "mine").unwrap();

        let Observed::At { dirty_files, .. } = source.inspect("r").unwrap() else {
            panic!("a checkout with edits is still a checkout");
        };

        assert_eq!(dirty_files, ["docs/guide/a.md", "notes.txt"]);
    }

    #[test]
    fn a_directory_refs_did_not_make_is_foreign() {
        let env = Env::new();
        let source = env.source();
        let checkouts = env.checkouts();
        fs::create_dir_all(checkouts.join("plain")).unwrap();
        fs::create_dir_all(checkouts.join("clone")).unwrap();
        git(&checkouts.join("clone"), &["init", "-q"]);
        fs::create_dir_all(checkouts.join("elsewhere")).unwrap();
        fs::write(
            checkouts.join("elsewhere/.git"),
            "gitdir: /somewhere/else/worktrees/x\n",
        )
        .unwrap();
        fs::write(checkouts.join("file"), "not a directory").unwrap();

        for id in ["plain", "clone", "elsewhere", "file"] {
            assert_eq!(source.inspect(id).unwrap(), Observed::Foreign, "{id}");
        }
    }

    #[test]
    fn a_checkout_whose_cache_was_wiped_is_dangling() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        fs::remove_dir_all(env.cache()).unwrap();

        assert_eq!(source.inspect("r").unwrap(), Observed::Dangling);
    }

    /// A second spelling of `dir`, as a symlink on Unix or the 8.3 short name on Windows (where
    /// a CI runner's temporary directory is spelled that way). Git writes the real one.
    fn other_spelling(dir: &std::path::Path) -> PathBuf {
        #[cfg(unix)]
        {
            let alias = dir.with_file_name("alias");
            std::os::unix::fs::symlink(dir, &alias).unwrap();
            alias
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let out = Command::new("cmd")
                .raw_arg(format!(
                    "/C for %I in (\"{}\") do @echo %~sI",
                    dir.display()
                ))
                .output()
                .unwrap();
            PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
        }
    }

    #[test]
    fn a_wiped_cache_is_dangling_whatever_way_its_root_is_spelled() {
        let (remote, env) = (Remote::new(), Env::new());
        let real = env.dir.path().join("a_directory_with_a_long_name");
        fs::create_dir_all(&real).unwrap();
        let source = GitSource::new(other_spelling(&real).join("cache"), env.checkouts());
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        fs::remove_dir_all(real.join("cache")).unwrap();

        assert_eq!(source.inspect("r").unwrap(), Observed::Dangling);
    }

    #[test]
    fn remove_takes_the_checkout_and_its_registration() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        fs::write(env.checkouts().join("r/notes.txt"), "mine").unwrap();

        source.remove("r").unwrap();

        assert!(!env.checkouts().join("r").exists());
        let worktrees = git(
            &env.cache_repo(&remote),
            &["worktree", "list", "--porcelain"],
        );
        assert_eq!(worktrees.matches("worktree ").count(), 1, "{worktrees}");
        source.remove("r").unwrap();
    }

    #[test]
    fn a_foreign_directory_is_never_removed_or_moved_onto() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        let theirs = env.checkouts().join("r");
        fs::create_dir_all(&theirs).unwrap();
        fs::write(theirs.join("keep.txt"), "theirs").unwrap();

        assert!(source.remove("r").is_err());
        assert!(
            source
                .materialise(at("r", &repo), &remote.pin(), MaterialiseOpts::default())
                .is_err()
        );

        assert_eq!(
            fs::read_to_string(theirs.join("keep.txt")).unwrap(),
            "theirs"
        );
    }

    #[test]
    fn a_dangling_checkout_is_removed_and_made_again() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        fs::remove_dir_all(env.cache()).unwrap();

        source.remove("r").unwrap();
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();

        assert_eq!(
            source.inspect("r").unwrap(),
            clean_at(&remote, &["docs/guide"])
        );
        assert!(env.files("r").contains("docs/guide/a.md"));
    }

    #[test]
    fn a_checkout_moves_to_another_commit() {
        let (mut remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        remote.advance();

        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();

        assert_eq!(
            source.inspect("r").unwrap(),
            clean_at(&remote, &["docs/guide"])
        );
        let checkout = env.checkouts().join("r");
        assert_eq!(
            fs::read_to_string(checkout.join("docs/guide/a.md")).unwrap(),
            "a, again"
        );
        assert!(checkout.join("docs/guide/new.md").exists());
    }

    #[test]
    fn a_move_offline_works_from_what_an_earlier_run_prefetched() {
        let (mut remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        remote.advance();
        let other = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "warm", &remote, &other, ONLINE).unwrap();

        materialise(&source, "r", &remote, &repo, OFFLINE).unwrap();

        assert_eq!(env.files("r"), env.files("warm"));
    }

    #[test]
    fn changing_the_paths_changes_the_files() {
        let (remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        materialise(
            &source,
            "r",
            &remote,
            &remote.repo(&["docs/guide"], &[]),
            ONLINE,
        )
        .unwrap();
        let wider = remote.repo(&["docs/guide", "src"], &[]);

        materialise(&source, "r", &remote, &wider, ONLINE).unwrap();

        assert!(env.files("r").contains("src/lib.rs"));
        assert_eq!(
            source.inspect("r").unwrap(),
            clean_at(&remote, &["docs/guide", "src"])
        );

        let whole = remote.repo(&[], &[]);
        materialise(&source, "r", &remote, &whole, ONLINE).unwrap();
        assert!(env.files("r").contains("docs/api/c.md"));
    }

    #[test]
    fn a_failed_move_offline_keeps_the_old_checkout_when_the_commit_is_missing() {
        let (mut remote, env) = (Remote::new(), Env::new());
        let source = env.source();
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &remote, &repo, ONLINE).unwrap();
        let old = remote.pin();
        remote.advance();

        assert_eq!(
            materialise(&source, "r", &remote, &repo, OFFLINE),
            Err("refs::git::not_cached".into())
        );

        let Observed::At { pin, .. } = source.inspect("r").unwrap() else {
            panic!("the old checkout stays");
        };
        assert_eq!(pin, old);
    }

    #[test]
    fn a_checkout_of_another_remote_is_replaced() {
        let (first, second, env) = (Remote::new(), Remote::new(), Env::new());
        let source = env.source();
        materialise(
            &source,
            "r",
            &first,
            &first.repo(&["docs/guide"], &[]),
            ONLINE,
        )
        .unwrap();

        materialise(&source, "r", &second, &second.repo(&["src"], &[]), ONLINE).unwrap();

        assert_eq!(source.inspect("r").unwrap(), clean_at(&second, &["src"]));
        let worktrees = git(
            &env.cache_repo(&first),
            &["worktree", "list", "--porcelain"],
        );
        assert_eq!(worktrees.matches("worktree ").count(), 1, "{worktrees}");
    }

    #[test]
    fn a_failed_switch_to_another_remote_keeps_the_old_checkout() {
        let (first, second, env) = (Remote::new(), Remote::new(), Env::new());
        let source = env.source();
        let guide = first.repo(&["docs/guide"], &[]);
        materialise(&source, "r", &first, &guide, ONLINE).unwrap();

        let result = materialise(&source, "r", &second, &second.repo(&["src"], &[]), OFFLINE);

        assert_eq!(result, Err("refs::git::not_cached".into()));
        assert_eq!(
            source.inspect("r").unwrap(),
            clean_at(&first, &["docs/guide"])
        );
    }

    #[test]
    fn a_moved_project_is_repaired_and_still_a_checkout() {
        let (remote, env) = (Remote::new(), Env::new());
        let repo = remote.repo(&["docs/guide"], &[]);
        materialise(&env.source(), "r", &remote, &repo, ONLINE).unwrap();
        let moved = env.dir.path().join("moved/.references");
        fs::create_dir_all(moved.parent().unwrap()).unwrap();
        fs::rename(env.checkouts(), &moved).unwrap();
        let source = GitSource::new(env.cache(), moved.clone());

        assert_eq!(
            source.inspect("r").unwrap(),
            clean_at(&remote, &["docs/guide"])
        );

        let worktrees = git(
            &env.cache_repo(&remote),
            &["worktree", "list", "--porcelain"],
        );
        // git lists the real spelling of the path, not necessarily the one the test built
        let here = dunce::canonicalize(moved.join("r")).unwrap();
        assert!(
            worktrees.contains(&here.to_str().unwrap().replace('\\', "/")),
            "{worktrees}"
        );
        git(&env.cache_repo(&remote), &["worktree", "prune"]);
        assert_eq!(
            source.inspect("r").unwrap(),
            clean_at(&remote, &["docs/guide"])
        );
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
