//! The policy behind the `paths` and `start` pickers, as data: what a Checkout holds, what
//! counts as a docs file, the order under the cap, and which selections are redundant.

use refs_cli::listing::{directories, drop_children, held, start_files, with_ticked};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_checkout_holds_the_root_the_way_to_a_path_and_everything_under_it() {
    let chosen = strings(&["packages/web"]);
    for (path, expected) in [
        ("README.md", true),
        ("packages/README.md", true),
        ("packages/web/a.md", true),
        ("packages/web/deep/a.md", true),
        ("packages/other/a.md", false),
        ("packages/webby/a.md", false),
        ("docs/a.md", false),
    ] {
        assert_eq!(held(path, &chosen), expected, "{path}");
    }
    assert!(held("docs/a.md", &[]));
}

#[test]
fn a_skipped_directory_hides_its_files_unless_it_was_chosen() {
    let nothing = strings(&[]);
    let examples = strings(&["examples"]);
    assert!(!held("examples/README.md", &nothing));
    assert!(!held(".github/README.md", &nothing));
    assert!(held("examples/README.md", &examples));
    assert!(held("examples/foo/README.md", &examples));
    assert!(!held("examples/node_modules/README.md", &examples));
}

#[test]
fn start_offers_docs_files_by_extension_and_readme_by_name() {
    let paths = strings(&[
        "README",
        "Readme.md",
        "a.MD",
        "b.mdx",
        "c.markdown",
        "d.rst",
        "e.adoc",
        "f.txt",
        "g.rs",
        "README.txt",
        "docs/md",
    ]);
    let (files, left) = start_files(&paths, &[]);
    assert_eq!(
        files,
        [
            "README",
            "README.txt",
            "Readme.md",
            "a.MD",
            "b.mdx",
            "c.markdown",
            "d.rst",
            "e.adoc"
        ]
    );
    assert_eq!(left, 0);
}

#[test]
fn directories_come_from_every_level_in_path_order() {
    let paths = strings(&["a/b/c.rs", "a/d.rs", "z.rs", "b/e.rs"]);
    assert_eq!(directories(&paths), (strings(&["a", "a/b", "b"]), 0));
}

#[test]
fn past_the_cap_the_shallowest_stay_and_the_rest_are_counted() {
    let mut paths: Vec<String> = (0..5000).map(|i| format!("d{i:04}/f.rs")).collect();
    paths.push("a/b/c/f.rs".into());
    let (dirs, left) = directories(&paths);
    assert_eq!(dirs.len(), 5000);
    assert_eq!(left, 3);
    assert!(!dirs.contains(&"a/b".to_string()) && dirs.contains(&"a".to_string()));
}

#[test]
fn a_selected_directory_makes_its_descendants_redundant() {
    let picked = strings(&["packages", "packages/a/b", "packages-x", "src/a", "src"]);
    assert_eq!(
        drop_children(picked),
        strings(&["packages", "packages-x", "src"])
    );
}

#[test]
fn a_ticked_option_the_list_lacks_is_added_in_path_order() {
    let options = strings(&["a", "c"]);
    assert_eq!(
        with_ticked(options, &strings(&["b", "a"])),
        strings(&["a", "b", "c"])
    );
}
