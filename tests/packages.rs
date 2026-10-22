use refs_cli::packages::{infer, manifest_paths};

fn files(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .collect()
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|p| p.to_string()).collect()
}

fn packages(pairs: &[(&str, &str)]) -> Vec<String> {
    infer(&files(pairs)).packages
}

#[test]
fn a_package_json_gives_its_name() {
    assert_eq!(
        packages(&[("package.json", r#"{ "name": "solid-js" }"#)]),
        ["solid-js"]
    );
}

#[test]
fn every_manifest_in_a_workspace_is_read_then_sorted_and_deduplicated() {
    assert_eq!(
        packages(&[
            ("packages/web/package.json", r#"{ "name": "@solidjs/web" }"#),
            ("packages/solid/package.json", r#"{ "name": "solid-js" }"#),
            ("packages/copy/package.json", r#"{ "name": "solid-js" }"#),
        ]),
        ["@solidjs/web", "solid-js"]
    );
}

#[test]
fn a_cargo_toml_gives_its_package_name_and_a_bare_workspace_gives_none() {
    assert_eq!(
        packages(&[
            ("Cargo.toml", "[workspace]\nmembers = [\"foo-bar\"]\n"),
            (
                "foo-bar/Cargo.toml",
                "[package]\nname = \"foo-bar\"\nversion = \"0.1.0\"\n"
            ),
        ]),
        ["foo-bar"]
    );
}

#[test]
fn a_pyproject_toml_gives_its_project_name() {
    assert_eq!(
        packages(&[("pyproject.toml", "[project]\nname = \"Pillow\"\n")]),
        ["Pillow"]
    );
}

#[test]
fn a_go_mod_gives_its_module_path() {
    assert_eq!(
        packages(&[
            (
                "go.mod",
                "// the module\nmodule github.com/spf13/cobra // trailing\n\ngo 1.22\n"
            ),
            ("quoted/go.mod", "module \"example.com/quoted\"\n"),
        ]),
        ["example.com/quoted", "github.com/spf13/cobra"]
    );
}

#[test]
fn private_unnamed_and_malformed_manifests_give_nothing() {
    assert_eq!(
        packages(&[
            ("package.json", r#"{ "name": "root", "private": true }"#),
            ("a/package.json", r#"{ "version": "1.0.0" }"#),
            ("b/package.json", "{ not json"),
            ("c/Cargo.toml", "[package\n"),
            (
                "d/package.json",
                r#"{ "name": "public", "private": false }"#
            ),
        ]),
        ["public"]
    );
}

#[test]
fn subpath_export_shims_with_invalid_npm_names_are_not_packages() {
    assert_eq!(
        packages(&[
            ("package.json", r#"{ "name": "solid-js" }"#),
            ("solid-js/h/package.json", r#"{ "name": "solid-js/h" }"#),
            ("solid-js/web/package.json", r#"{ "name": "solid-js/web" }"#),
            ("packages/web/package.json", r#"{ "name": "@solidjs/web" }"#),
            ("a/package.json", r#"{ "name": "_hidden" }"#),
            ("b/package.json", r#"{ "name": "@scope/.dot" }"#),
            ("c/package.json", r#"{ "name": " spaced" }"#),
            ("d/package.json", r#"{ "name": "node_modules" }"#),
            // Legacy names that npm still serves stay.
            ("e/package.json", r#"{ "name": "JSONStream" }"#),
        ]),
        ["@solidjs/web", "JSONStream", "solid-js"]
    );
}

#[test]
fn a_cargo_package_that_is_not_published_is_not_a_package() {
    assert_eq!(
        packages(&[
            (
                "Cargo.toml",
                "[package]\nname = \"lib\"\nversion = \"0.1.0\"\n"
            ),
            (
                "xtask/Cargo.toml",
                "[package]\nname = \"xtask\"\npublish = false\n"
            ),
            (
                "none/Cargo.toml",
                "[package]\nname = \"none\"\npublish = []\n"
            ),
            (
                "private/Cargo.toml",
                "[package]\nname = \"elsewhere\"\npublish = [\"my-registry\"]\n"
            ),
        ]),
        ["elsewhere", "lib"]
    );
}

#[test]
fn the_description_is_the_root_manifests_on_one_line() {
    let description = |pairs: &[(&str, &str)]| infer(&files(pairs)).description;
    assert_eq!(
        description(&[
            (
                "packages/a/package.json",
                r#"{ "name": "a", "description": "nested" }"#
            ),
            (
                "package.json",
                r#"{ "name": "r", "private": true, "description": " A UI\nlibrary " }"#
            ),
        ])
        .as_deref(),
        Some("A UI library")
    );
    assert_eq!(
        description(&[(
            "Cargo.toml",
            "[package]\nname = \"c\"\ndescription = \"crate\"\n"
        )])
        .as_deref(),
        Some("crate")
    );
    assert_eq!(
        description(&[(
            "pyproject.toml",
            "[project]\nname = \"p\"\ndescription = \"py\"\n"
        )])
        .as_deref(),
        Some("py")
    );
    assert_eq!(
        description(&[("a/package.json", r#"{ "description": "x" }"#)]),
        None
    );
}

#[test]
fn only_manifests_that_will_be_in_the_checkout_are_chosen() {
    let all = paths(&[
        "package.json",
        "README.md",
        "packages/package.json",
        "packages/web/package.json",
        "packages/web/src/Cargo.toml",
        "packages/solid/package.json",
        "other/package.json",
    ]);
    // Cone mode: the root's files, the files along the way, and everything under a path.
    let (chosen, left) = manifest_paths(&all, &paths(&["packages/web"]));
    assert_eq!(
        chosen,
        [
            "package.json",
            "packages/package.json",
            "packages/web/package.json",
            "packages/web/src/Cargo.toml",
        ]
    );
    assert_eq!(left, 0);
    // No paths is the whole repo.
    assert_eq!(manifest_paths(&all, &[]).0.len(), 6);
}

#[test]
fn dependency_build_example_and_dot_directories_are_not_chosen() {
    let mut all = paths(&["package.json"]);
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
        "a/node_modules/b",
    ] {
        all.push(format!("{skipped}/package.json"));
    }
    assert_eq!(manifest_paths(&all, &[]).0, ["package.json"]);
}

#[test]
fn at_most_200_manifests_are_chosen_shallowest_first_and_the_rest_counted() {
    let mut all: Vec<String> = (0..150).map(|i| format!("a/b/c{i}/package.json")).collect();
    all.extend((0..100).map(|i| format!("p{i}/package.json")));
    let (chosen, left) = manifest_paths(&all, &[]);
    assert_eq!(chosen.len(), 200);
    assert_eq!(left, 50);
    assert!(chosen.iter().take(100).all(|p| p.starts_with('p')));
}
