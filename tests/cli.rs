//! The CLI at its seam: `cli::run` in-process against the fake `Source`, and the built
//! binary for what only a process shows (clap's exit code, what reaches stderr). The binary
//! cannot take the fake, so its tests stop before any Source call.

mod common;

use common::git;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;

use refs_cli::cli::run;
use refs_cli::source::Observed;
use refs_cli::source::fake::{FakeSource, Method};
use tempfile::TempDir;

const AB: &str = r#"
[repos.a]
url = "https://github.com/o/a"
[repos.b]
url = "https://github.com/o/b"
"#;

const SHA_1: &str = "1111111111111111111111111111111111111111";
const SHA_2: &str = "2222222222222222222222222222222222222222";

struct Project {
    dir: TempDir,
    source: FakeSource,
}

impl Project {
    fn new(config: &str) -> Project {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git/info")).unwrap();
        fs::write(dir.path().join("refs.toml"), config).unwrap();
        Project {
            dir,
            source: FakeSource::new(),
        }
    }

    /// Run `refs <args>` from a subdirectory, so root discovery is part of every test.
    fn run(&self, args: &[&str]) -> u8 {
        let sub = self.dir.path().join("sub");
        fs::create_dir_all(&sub).unwrap();
        let args = std::iter::once("refs").chain(args.iter().copied());
        run(args.map(Into::into), &sub, |_, _| {
            Ok(Box::new(&self.source))
        })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn lock_text(&self) -> Option<String> {
        fs::read_to_string(self.path("refs.lock")).ok()
    }

    /// The locked commit of `id`, read from the lock file's text.
    fn sha(&self, id: &str) -> String {
        let text = self.lock_text().unwrap();
        let entry = text
            .split("[[repo]]")
            .find(|e| e.contains(&format!("id = \"{id}\"")))
            .unwrap();
        let line = entry.lines().find(|l| l.starts_with("sha = ")).unwrap();
        line.trim_start_matches("sha = ").trim_matches('"').into()
    }
}

#[test]
fn a_sync_that_is_in_sync_exits_0() {
    let p = Project::new(AB);

    assert_eq!(p.run(&["sync"]), 0);
    assert_eq!(p.run(&["sync", "--check"]), 0);
}

#[test]
fn check_on_a_fresh_project_exits_3_and_writes_nothing() {
    let p = Project::new(AB);

    assert_eq!(p.run(&["sync", "--check"]), 3);

    assert_eq!(p.lock_text(), None);
    assert!(!p.path("AGENTS.md").exists());
    assert!(!p.path(".references").exists());
}

#[test]
fn a_failed_repo_exits_1() {
    let p = Project::new(AB);
    p.source.fail("a", Method::Materialise, "no space left");

    assert_eq!(p.run(&["sync"]), 1);
}

#[test]
fn a_refusal_exits_1_with_or_without_check() {
    let p = Project::new(AB);
    assert_eq!(p.run(&["sync"]), 0);
    p.source.seed("a", Observed::Foreign);

    assert_eq!(p.run(&["sync", "--check"]), 1);
    assert_eq!(p.run(&["sync"]), 1);
}

#[test]
fn lock_writes_the_lock_and_touches_no_checkout() {
    let p = Project::new(AB);

    assert_eq!(p.run(&["lock"]), 0);

    assert!(p.lock_text().is_some());
    assert!(!p.path("AGENTS.md").exists());
}

#[test]
fn a_bare_upgrade_reaches_the_plan_as_all_and_ids_as_ids() {
    let p = Project::new(AB);
    p.source.set_commit("a", SHA_1);
    p.source.set_commit("b", SHA_1);
    assert_eq!(p.run(&["lock"]), 0);
    p.source.set_commit("a", SHA_2);
    p.source.set_commit("b", SHA_2);

    assert_eq!(p.run(&["lock"]), 0);
    assert_eq!((p.sha("a"), p.sha("b")), (SHA_1.into(), SHA_1.into()));

    assert_eq!(p.run(&["lock", "--upgrade", "a"]), 0);
    assert_eq!((p.sha("a"), p.sha("b")), (SHA_2.into(), SHA_1.into()));

    p.source.set_commit("a", SHA_1);
    p.source.set_commit("b", SHA_2);
    assert_eq!(p.run(&["lock", "--upgrade"]), 0);
    assert_eq!((p.sha("a"), p.sha("b")), (SHA_1.into(), SHA_2.into()));
}

#[test]
fn a_usage_error_exits_2() {
    let p = Project::new(AB);

    assert_eq!(p.run(&["sync", "--frobnicate"]), 2);
    assert_eq!(p.run(&[]), 2);
}

#[test]
fn a_config_error_exits_1_and_writes_nothing() {
    let p = Project::new("[repos.a]\nurl = \"ftp://x/y\"\n");

    assert_eq!(p.run(&["sync"]), 1);

    assert_eq!(p.lock_text(), None);
}

#[test]
fn a_missing_refs_toml_exits_1() {
    let dir = TempDir::new().unwrap();

    let code = run(["refs", "sync"].map(Into::into), dir.path(), |_, _| {
        Ok(Box::new(FakeSource::new()))
    });

    assert_eq!(code, 1);
}

#[test]
fn a_references_dir_symlink_that_leaves_the_project_is_rejected_before_any_write() {
    let p = Project::new(AB);
    let outside = TempDir::new().unwrap();
    symlink(outside.path(), p.path(".references")).unwrap();

    assert_eq!(p.run(&["sync"]), 1);

    assert_eq!(p.lock_text(), None);
    assert!(!p.path("AGENTS.md").exists());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn project_flag_overrides_the_directory_search() {
    let p = Project::new(AB);
    let elsewhere = TempDir::new().unwrap();
    let root = p.dir.path().to_str().unwrap();

    let code = run(
        ["refs", "--project", root, "lock"].map(Into::into),
        elsewhere.path(),
        |_, _| Ok(Box::new(&p.source)),
    );

    assert_eq!(code, 0);
    assert!(p.lock_text().is_some());
}

fn binary(dir: &TempDir, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_refs"))
        .args(args)
        .current_dir(dir.path())
        // The real `GitSource` fills a Cache; keep it out of the user's.
        .env("XDG_CACHE_HOME", dir.path().join("cache"))
        .output()
        .unwrap()
}

#[test]
fn the_binary_prints_every_config_error_and_exits_1() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("refs.toml"),
        "[repos.a]\nurl = \"ftp://x/y\"\n[repos.b]\nurl = \"https://h/b\"\nref = \"-x\"\n",
    )
    .unwrap();

    let out = binary(&dir, &["--no-color", "sync"]);

    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_eq!(stderr.matches("refs::config::").count(), 2, "{stderr}");
}

#[test]
fn the_binary_exits_2_on_a_usage_error_and_0_on_help() {
    let dir = TempDir::new().unwrap();

    assert_eq!(binary(&dir, &["nope"]).status.code(), Some(2));
    assert_eq!(binary(&dir, &["--help"]).status.code(), Some(0));
}

#[test]
fn the_binary_locks_with_git_and_reports_an_unknown_ref() {
    let remote = TempDir::new().unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ],
    ] {
        let status = Command::new("git")
            .current_dir(remote.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let project = TempDir::new().unwrap();
    let config = |git_ref: &str| {
        let url = format!("file://{}", remote.path().display());
        fs::write(
            project.path().join("refs.toml"),
            format!("[repos.r]\nurl = \"{url}\"\nref = \"{git_ref}\"\n"),
        )
        .unwrap();
    };

    config("main");
    let out = binary(&project, &["--no-color", "lock"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(project.path().join("refs.lock").exists());

    config("nope");
    let out = binary(&project, &["--no-color", "lock"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("refs::git::ref_not_found"), "{stderr}");
}

#[test]
fn the_binary_syncs_recreates_a_wiped_cache_and_guards_edits() {
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "-q", "-b", "main"]);
    git(remote.path(), &["config", "uploadpack.allowFilter", "true"]);
    fs::create_dir_all(remote.path().join("docs")).unwrap();
    fs::write(remote.path().join("docs/a.md"), "a").unwrap();
    git(remote.path(), &["add", "."]);
    git(remote.path(), &["commit", "-q", "-m", "x"]);
    let project = TempDir::new().unwrap();
    git(project.path(), &["init", "-q"]);
    fs::write(
        project.path().join("refs.toml"),
        format!(
            "[repos.r]\nurl = \"file://{}\"\nref = \"main\"\npaths = [\"docs\"]\n",
            remote.path().display()
        ),
    )
    .unwrap();
    let sync = |args: &[&str]| {
        let out = binary(&project, &[&["--no-color", "sync"], args].concat());
        (out.status.code(), String::from_utf8(out.stderr).unwrap())
    };
    let checkout = project.path().join(".references/r");

    assert_eq!(sync(&[]).0, Some(0));
    assert!(checkout.join("docs/a.md").exists());
    assert_eq!(sync(&["--check"]).0, Some(0));

    fs::remove_dir_all(project.path().join("cache")).unwrap();
    let (code, stderr) = sync(&[]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.contains("refs::sync::recreated"), "{stderr}");
    assert!(checkout.join("docs/a.md").exists());

    fs::write(checkout.join("docs/a.md"), "edited").unwrap();
    fs::write(
        project.path().join("refs.toml"),
        format!(
            "[repos.r]\nurl = \"file://{}\"\nref = \"main\"\npaths = []\n",
            remote.path().display()
        ),
    )
    .unwrap();
    let (code, stderr) = sync(&[]);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("refs::sync::dirty_checkout"), "{stderr}");
    assert_eq!(
        fs::read_to_string(checkout.join("docs/a.md")).unwrap(),
        "edited"
    );

    let (code, stderr) = sync(&["--force"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(fs::read_to_string(checkout.join("docs/a.md")).unwrap(), "a");
}

#[test]
fn disabling_a_repo_or_its_group_then_syncing_drops_its_lock_entry_and_entry() {
    let p = Project::new(
        r#"
[groups.g]
name = "Group G"
[repos.a]
url = "https://github.com/o/a"
group = "g"
[repos.b]
url = "https://github.com/o/b"
"#,
    );
    assert_eq!(p.run(&["sync"]), 0);
    let block = fs::read_to_string(p.path("AGENTS.md")).unwrap();
    assert!(block.contains("[a @") && block.contains("[b @") && block.contains("Group G"));

    assert_eq!(p.run(&["disable", "b"]), 0);
    assert_eq!(p.run(&["sync", "--check"]), 3, "the lock still has b");
    assert_eq!(p.run(&["sync"]), 0);
    let (lock, block) = (
        p.lock_text().unwrap(),
        fs::read_to_string(p.path("AGENTS.md")).unwrap(),
    );
    assert!(!lock.contains("\"b\"") && lock.contains("\"a\""), "{lock}");
    assert!(!block.contains("[b @") && block.contains("[a @"), "{block}");

    assert_eq!(p.run(&["disable", "g", "--group"]), 0);
    assert_eq!(p.run(&["sync"]), 0);
    let (lock, block) = (
        p.lock_text().unwrap(),
        fs::read_to_string(p.path("AGENTS.md")).unwrap(),
    );
    assert!(!lock.contains("\"a\""), "{lock}");
    assert!(
        !block.contains("Group G") && !block.contains("[a @"),
        "{block}"
    );
    assert_eq!(p.run(&["sync", "--check"]), 0);
}
