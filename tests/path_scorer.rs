//! The order the pickers list matches in (see `prompt::path_scorer`), for what a person typed
//! while narrowing a real repo's directories.

use refs_cli::prompt::path_scorer;

/// `options` that match `typed`, best first, as `inquire` orders them.
fn ranked<'a>(typed: &str, options: &'a [String]) -> Vec<&'a str> {
    let score = path_scorer();
    let mut found: Vec<(i64, &str)> = options
        .iter()
        .enumerate()
        .filter_map(|(i, o)| Some((score(typed, o, o, i)?, o.as_str())))
        .collect();
    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    found.into_iter().map(|(_, o)| o).collect()
}

fn solid() -> Vec<String> {
    [
        "packages",
        "packages/babel-preset-solid",
        "packages/solid",
        "packages/solid-element",
        "packages/solid-ssr",
        "packages/solid-ssr/examples",
        "packages/solid/h",
        "packages/solid/h/jsx-dev-runtime",
        "packages/solid/web",
    ]
    .map(String::from)
    .to_vec()
}

#[test]
fn a_typed_prefix_lists_the_directory_before_what_is_inside_it() {
    let options = solid();
    for typed in ["packages/s", "packages/solid", "packages/sol"] {
        assert_eq!(ranked(typed, &options)[0], "packages/solid", "{typed}");
    }
}

#[test]
fn nothing_typed_keeps_path_order_and_a_subsequence_still_matches() {
    let options = solid();
    assert_eq!(ranked("", &options), options);
    assert!(ranked("pksol", &options).contains(&"packages/solid"));
}
