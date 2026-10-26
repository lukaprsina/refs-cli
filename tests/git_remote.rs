//! Where the git cache lives, tested as data: no git is run.

use refs_cli::source::git::cache::cache_root;
use std::path::PathBuf;

#[test]
fn xdg_cache_home_wins() {
    // `/x/cache` has no drive letter, so it is not absolute on Windows.
    let xdg = std::env::temp_dir().join("cache");
    let local = std::env::temp_dir().join("local");
    assert_eq!(
        cache_root(xdg.to_str(), local.to_str(), Some("/home/u")),
        Some(xdg.join("refs"))
    );
}

#[test]
fn local_app_data_beats_home() {
    let local = std::env::temp_dir().join("local");
    assert_eq!(
        cache_root(None, local.to_str(), Some("/home/u")),
        Some(local.join("refs"))
    );
}

#[test]
fn the_default_is_dot_cache_in_home() {
    assert_eq!(
        cache_root(None, None, Some("/home/u")),
        Some(PathBuf::from("/home/u/.cache/refs"))
    );
}

#[test]
fn a_relative_xdg_cache_home_or_local_app_data_is_ignored() {
    assert_eq!(
        cache_root(Some("cache"), Some("local"), Some("/home/u")),
        Some(PathBuf::from("/home/u/.cache/refs"))
    );
}

#[test]
fn with_none_of_them_there_is_no_cache() {
    assert_eq!(cache_root(None, None, None), None);
}
