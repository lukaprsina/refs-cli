//! The config-only commands through `cli::run`. They take no `Source`: the factory below
//! panics, so a command that reached for one (and so for git) would fail its test.

use std::fs;
use std::path::Path;

use refs_cli::cli::run;
use refs_cli::diagnostic::SourceError;
use refs_cli::source::Source;
use tempfile::TempDir;

const CONFIG: &str = r#"# Pinned references.
[settings]
agents_files = ["AGENTS.md"] # the one agents read

# The core.
[repos.solid]
url = "https://github.com/solidjs/solid"
"#;

fn project(config: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("refs.toml"), config).unwrap();
    dir
}

fn refs(dir: &Path, args: &[&str]) -> u8 {
    let args = std::iter::once("refs").chain(args.iter().copied());
    run(
        args.map(Into::into),
        dir,
        |_, _| -> Result<Box<dyn Source>, SourceError> {
            panic!("a config-only command asked for a Source")
        },
    )
}

fn config_text(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join("refs.toml")).unwrap()
}

#[test]
fn add_then_remove_leave_refs_toml_byte_identical() {
    let dir = project(CONFIG);

    let url = "https://github.com/solidjs/solid-router";
    assert_eq!(
        refs(
            dir.path(),
            &["add", url, "--ref", "next", "--paths", "src", "docs"]
        ),
        0
    );
    let added = config_text(&dir);
    assert!(
        added.contains("[repos.solid-router]") && added.contains("paths = [\"src\", \"docs\"]"),
        "{added}"
    );

    assert_eq!(refs(dir.path(), &["remove", "solid-router"]), 0);
    assert_eq!(config_text(&dir), CONFIG);
}

#[test]
fn a_rejected_add_exits_1_and_leaves_refs_toml_untouched() {
    let dir = project(CONFIG);

    assert_eq!(refs(dir.path(), &["add", "http://github.com/o/r"]), 1);
    assert_eq!(refs(dir.path(), &["add", "https://github.com/o/solid"]), 1);
    assert_eq!(refs(dir.path(), &["remove", "nope"]), 1);

    assert_eq!(config_text(&dir), CONFIG);
}

#[test]
fn list_reads_the_config_only() {
    let dir = project(CONFIG);

    assert_eq!(refs(dir.path(), &["list"]), 0);

    // No lock, no checkout, no agent file were needed or made.
    let mut names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["refs.toml"]);
}
