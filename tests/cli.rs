//! The CLI at its seam: `cli::run` in-process against the fake `Source`, and the built
//! binary for what only a process shows (clap's exit code, what reaches stderr). The binary
//! cannot take the fake, so its tests stop before any Source call.

mod common;

use common::git;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;

use refs_cli::cli::run;
use refs_cli::source::Observed;
use refs_cli::source::fake::{Call, FakeSource, Method};
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
fn a_usage_error_is_styled_only_for_a_terminal() {
    use refs_cli::cli::{Terminal, run_on, run_with};
    use refs_cli::source::Source;
    let p = Project::new(AB);
    let args = || ["refs", "sync", "--frobnicate"].map(std::ffi::OsString::from);
    let source = |_: &_, _: &_| Ok(Box::new(&p.source) as Box<dyn Source>);

    let (mut out, mut plain) = (Vec::new(), Vec::new());
    assert_eq!(
        run_with(args(), p.dir.path(), source, &mut out, &mut plain),
        2
    );
    let (mut out, mut styled) = (Vec::new(), Vec::new());
    let terminal = Terminal {
        out: false,
        err: true,
    };
    assert_eq!(
        run_on(
            args(),
            p.dir.path(),
            source,
            &mut out,
            &mut styled,
            terminal
        ),
        2
    );

    let (plain, styled) = (
        String::from_utf8(plain).unwrap(),
        String::from_utf8(styled).unwrap(),
    );
    assert!(!plain.contains('\x1b'), "{plain:?}");
    assert!(styled.contains('\x1b'), "{styled:?}");
    assert!(plain.contains("--frobnicate") && styled.contains("--frobnicate"));
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
#[cfg(unix)]
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
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_WORK_TREE")
            .current_dir(remote.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let project = TempDir::new().unwrap();
    let config = |git_ref: &str| {
        let url = common::file_url(remote.path());
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
            "[repos.r]\nurl = \"{}\"\nref = \"main\"\npaths = [\"docs\"]\n",
            common::file_url(remote.path())
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
            "[repos.r]\nurl = \"{}\"\nref = \"main\"\npaths = []\n",
            common::file_url(remote.path())
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
    assert_eq!(p.run(&["sync", "--check"]), 0, "disable synced already");
    let (lock, block) = (
        p.lock_text().unwrap(),
        fs::read_to_string(p.path("AGENTS.md")).unwrap(),
    );
    assert!(!lock.contains("\"b\"") && lock.contains("\"a\""), "{lock}");
    assert!(p.source.calls().contains(&Call::Remove("b".into())));
    assert!(!block.contains("[b @") && block.contains("[a @"), "{block}");

    assert_eq!(p.run(&["disable", "g", "--group"]), 0);
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

#[test]
fn a_group_whose_repos_are_all_disabled_has_no_heading() {
    let p = Project::new(
        "[groups.g]\nname = \"Group G\"\n[repos.a]\nurl = \"https://github.com/o/a\"\ngroup = \"g\"\nenabled = false\n[repos.b]\nurl = \"https://github.com/o/b\"\n",
    );
    assert_eq!(p.run(&["sync"]), 0);
    let block = fs::read_to_string(p.path("AGENTS.md")).unwrap();
    assert!(
        !block.contains("Group G") && block.contains("[b @"),
        "{block}"
    );
}

#[test]
fn init_then_sync_check_passes_with_no_user_level_config() {
    let dir = TempDir::new().unwrap();
    let home = dir.path().join("home");
    fs::create_dir(&home).unwrap();
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_refs"))
            .args(args)
            .current_dir(&project)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .code()
    };
    assert_eq!(run(&["init"]), Some(0));
    assert_eq!(run(&["sync"]), Some(0));
    assert_eq!(run(&["sync", "--check"]), Some(0));
}

#[test]
fn add_locks_and_syncs_the_new_repo_in_one_step() {
    let p = Project::new(AB);
    assert_eq!(p.run(&["sync"]), 0);

    assert_eq!(
        p.run(&["add", "https://github.com/o/c", "--ref", "next"]),
        0
    );

    assert!(
        fs::read_to_string(p.path("refs.toml"))
            .unwrap()
            .contains("[repos.c]")
    );
    assert!(p.lock_text().unwrap().contains("id = \"c\""));
    assert!(
        fs::read_to_string(p.path("AGENTS.md"))
            .unwrap()
            .contains("[c @")
    );
    assert_eq!(p.run(&["sync", "--check"]), 0);
}

#[test]
fn an_add_that_fails_to_lock_exits_1_and_changes_no_file() {
    let p = Project::new(AB);
    assert_eq!(p.run(&["sync"]), 0);
    let (config, lock) = (fs::read(p.path("refs.toml")).unwrap(), p.lock_text());
    p.source.fail("c", Method::Resolve, "no such ref");

    assert_eq!(p.run(&["add", "https://github.com/o/c"]), 1);

    assert_eq!(fs::read(p.path("refs.toml")).unwrap(), config);
    assert_eq!(p.lock_text(), lock);
}

#[test]
fn an_add_whose_checkout_fails_keeps_the_edit_and_exits_1() {
    let p = Project::new(AB);
    assert_eq!(p.run(&["sync"]), 0);
    p.source.fail("c", Method::Materialise, "network down");

    assert_eq!(p.run(&["add", "https://github.com/o/c"]), 1);

    assert!(
        fs::read_to_string(p.path("refs.toml"))
            .unwrap()
            .contains("[repos.c]")
    );
    assert!(p.lock_text().unwrap().contains("id = \"c\""));
    p.source.heal("c", Method::Materialise);
    assert_eq!(p.run(&["sync"]), 0);
}

#[test]
fn remove_syncs_too_and_no_sync_leaves_that_to_the_user() {
    let p = Project::new(AB);
    assert_eq!(p.run(&["sync"]), 0);

    assert_eq!(p.run(&["remove", "a", "--no-sync"]), 0);
    assert_eq!(p.run(&["sync", "--check"]), 3, "the lock still has a");

    assert_eq!(p.run(&["remove", "b"]), 0);
    assert_eq!(p.run(&["sync", "--check"]), 0);
    assert!(!p.lock_text().unwrap().contains("id = "));
}

#[test]
fn list_status_succeeds_through_the_source_and_plain_list_does_too() {
    let p = Project::new(AB);
    assert_eq!(p.run(&["sync"]), 0);
    assert_eq!(p.run(&["list", "--status"]), 0);
    assert_eq!(p.run(&["list"]), 0);
}

#[test]
fn enabling_what_is_enabled_changes_no_config_and_still_syncs() {
    let p = Project::new(AB);
    let config = fs::read(p.path("refs.toml")).unwrap();

    assert_eq!(p.run(&["enable", "a"]), 0);

    assert_eq!(fs::read(p.path("refs.toml")).unwrap(), config);
    assert!(
        p.lock_text().unwrap().contains("id = \"a\""),
        "it caught the project up"
    );
}

#[test]
fn an_enable_whose_repo_cannot_be_locked_exits_1_and_changes_no_file() {
    let p = Project::new(&format!("{AB}enabled = false\n"));
    assert_eq!(p.run(&["sync"]), 0);
    let (config, lock) = (fs::read(p.path("refs.toml")).unwrap(), p.lock_text());
    p.source.fail("b", Method::Resolve, "gone");

    assert_eq!(p.run(&["enable", "b"]), 1);

    assert_eq!(fs::read(p.path("refs.toml")).unwrap(), config);
    assert_eq!(p.lock_text(), lock);
}

#[test]
fn quiet_still_syncs_after_an_edit() {
    let p = Project::new(AB);

    assert_eq!(p.run(&["-q", "add", "https://github.com/o/c"]), 0);

    assert!(p.lock_text().unwrap().contains("id = \"c\""));
}

#[test]
fn list_status_works_with_no_lock_yet() {
    let p = Project::new(AB);
    assert_eq!(p.run(&["list", "--status"]), 0);
    assert_eq!(p.lock_text(), None, "list writes nothing");
}

#[test]
fn a_disable_with_nothing_locked_is_blocked_by_a_broken_repo_and_writes_nothing() {
    let p = Project::new(&format!(
        "{AB}
[repos.c]
url = \"https://github.com/o/c\"
"
    ));
    p.source.fail("b", Method::Resolve, "gone");
    let before = fs::read_to_string(p.path("refs.toml")).unwrap();

    assert_eq!(p.run(&["disable", "a"]), 1);

    assert_eq!(fs::read_to_string(p.path("refs.toml")).unwrap(), before);
    assert_eq!(p.lock_text(), None);
}

#[test]
fn removing_the_broken_repo_succeeds() {
    let p = Project::new(AB);
    p.source.fail("b", Method::Resolve, "gone");

    assert_eq!(p.run(&["remove", "b"]), 0);

    assert!(
        !fs::read_to_string(p.path("refs.toml"))
            .unwrap()
            .contains("[repos.b]")
    );
    assert_eq!(p.run(&["sync", "--check"]), 0);
}

#[test]
fn add_with_a_missing_group_creates_it_and_remove_takes_it_away_again() {
    let p = Project::new(AB);
    assert_eq!(p.run(&["sync"]), 0);
    let before = fs::read(p.path("refs.toml")).unwrap();

    assert_eq!(
        p.run(&["add", "https://github.com/o/c", "--group", "extra"]),
        0
    );
    let config = fs::read_to_string(p.path("refs.toml")).unwrap();
    assert!(config.contains("[groups.extra]"), "{config}");
    assert!(
        fs::read_to_string(p.path("AGENTS.md"))
            .unwrap()
            .contains("### extra")
    );

    assert_eq!(p.run(&["remove", "c"]), 0);
    assert_eq!(fs::read(p.path("refs.toml")).unwrap(), before);
}
