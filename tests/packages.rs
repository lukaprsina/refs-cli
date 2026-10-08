use std::fs;
use std::path::Path;

use refs_cli::packages::infer_packages;

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn a_package_json_gives_its_name() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "package.json", r#"{ "name": "solid-js" }"#);
    assert_eq!(infer_packages(dir.path()), ["solid-js"]);
}

#[test]
fn every_manifest_in_a_workspace_is_read_then_sorted_and_deduplicated() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "packages/web/package.json",
        r#"{ "name": "@solidjs/web" }"#,
    );
    write(
        dir.path(),
        "packages/solid/package.json",
        r#"{ "name": "solid-js" }"#,
    );
    write(
        dir.path(),
        "packages/copy/package.json",
        r#"{ "name": "solid-js" }"#,
    );
    assert_eq!(infer_packages(dir.path()), ["@solidjs/web", "solid-js"]);
}

#[test]
fn a_cargo_toml_gives_its_package_name_and_a_bare_workspace_gives_none() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        "[workspace]\nmembers = [\"foo-bar\"]\n",
    );
    write(
        dir.path(),
        "foo-bar/Cargo.toml",
        "[package]\nname = \"foo-bar\"\nversion = \"0.1.0\"\n",
    );
    assert_eq!(infer_packages(dir.path()), ["foo-bar"]);
}

#[test]
fn a_pyproject_toml_gives_its_project_name() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "pyproject.toml",
        "[project]\nname = \"Pillow\"\n",
    );
    assert_eq!(infer_packages(dir.path()), ["Pillow"]);
}

#[test]
fn a_go_mod_gives_its_module_path() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "go.mod",
        "// the module\nmodule github.com/spf13/cobra // trailing\n\ngo 1.22\n",
    );
    write(
        dir.path(),
        "quoted/go.mod",
        "module \"example.com/quoted\"\n",
    );
    assert_eq!(
        infer_packages(dir.path()),
        ["example.com/quoted", "github.com/spf13/cobra"]
    );
}

#[test]
fn dependency_build_example_and_dot_directories_are_not_read() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "package.json", r#"{ "name": "kept" }"#);
    for skipped in [
        "node_modules/dep",
        "target",
        "vendor/x",
        "dist",
        "examples/basic",
        "fixtures/f",
        "test/t",
        "tests/t",
        ".github/actions",
    ] {
        write(
            dir.path(),
            &format!("{skipped}/package.json"),
            r#"{ "name": "skipped" }"#,
        );
    }
    assert_eq!(infer_packages(dir.path()), ["kept"]);
}

#[test]
fn private_unnamed_and_malformed_manifests_give_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "package.json",
        r#"{ "name": "root", "private": true }"#,
    );
    write(dir.path(), "a/package.json", r#"{ "version": "1.0.0" }"#);
    write(dir.path(), "b/package.json", "{ not json");
    write(dir.path(), "c/Cargo.toml", "[package\n");
    write(
        dir.path(),
        "d/package.json",
        r#"{ "name": "public", "private": false }"#,
    );
    assert_eq!(infer_packages(dir.path()), ["public"]);
}

#[test]
fn a_sparse_checkout_gives_the_manifests_it_has() {
    let dir = tempfile::tempdir().unwrap();
    // `paths = ["packages/web"]`: the root's files, and only that path below it.
    write(
        dir.path(),
        "package.json",
        r#"{ "name": "solid-monorepo-docs" }"#,
    );
    write(
        dir.path(),
        "packages/web/package.json",
        r#"{ "name": "@solidjs/web" }"#,
    );
    assert_eq!(
        infer_packages(dir.path()),
        ["@solidjs/web", "solid-monorepo-docs"]
    );
}
