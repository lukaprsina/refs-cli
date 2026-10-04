//! `refs list` through the real binary: what lands on stdout.

use std::fs;
use std::process::Command;

use tempfile::TempDir;

const CONFIG: &str = r#"
[repos.router]
url = "https://github.com/solidjs/solid-router"
ref = "next"
paths = ["src", "docs"]

[repos.old]
url = "https://github.com/o/old"
enabled = false
"#;

fn refs(config: &str, args: &[&str]) -> (String, String) {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("refs.toml"), config).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_refs"))
        .args(args)
        .arg("--project")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn list_prints_one_aligned_line_per_repo_on_stdout_without_a_group_heading() {
    let (stdout, stderr) = refs(CONFIG, &["list"]);
    assert_eq!(
        stdout,
        "  router  https://github.com/solidjs/solid-router  next  src, docs     on
  old     https://github.com/o/old                 HEAD  (whole repo)  off
"
    );
    assert_eq!(stderr, "");
}

#[test]
fn list_status_keeps_the_base_columns_and_adds_the_sha_and_state() {
    let (stdout, _) = refs(CONFIG, &["list", "--status", "--no-color"]);
    assert_eq!(
        stdout,
        "  router  https://github.com/solidjs/solid-router  next  src, docs     on   -  not locked
  old     https://github.com/o/old                 HEAD  (whole repo)  off  -  disabled
"
    );
}
