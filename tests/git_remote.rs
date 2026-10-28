//! The pure half of git resolution that nothing else pins (input checks, URL normalisation and the
//! cache name, ref selection), tested as data: no git is run.

use miette::Diagnostic;
use refs_cli::source::git::cache::cache_root;
use refs_cli::source::git::remote::{
    cache_dir_name, check_input, check_version, normalise_url, select_head, select_ref,
};

const URL: &str = "https://example.com/o/r";
const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn code(e: impl Diagnostic) -> String {
    e.code().expect("a code").to_string()
}

mod select_ref {
    use super::*;

    #[test]
    fn a_ref_that_only_tail_matches_is_ignored() {
        let out = format!("{B}\trefs/x/refs/heads/main\n{C}\trefs/pull/refs/tags/main\n");
        let e = select_ref(URL, "main", &out).unwrap_err();
        assert_eq!(code(e), "refs::git::ref_not_found");
    }

    #[test]
    fn nothing_found_is_an_error() {
        let e = select_ref(URL, "main", "").unwrap_err();
        assert_eq!(code(e), "refs::git::ref_not_found");
    }

    #[test]
    fn two_different_commits_for_one_ref_is_ambiguous() {
        let out = format!("{A}\trefs/heads/main\n{B}\trefs/heads/main\n");
        let e = select_ref(URL, "main", &out).unwrap_err();
        assert_eq!(code(e), "refs::git::ambiguous_ref");
    }
}

mod select_head {
    use super::*;

    #[test]
    fn a_remote_without_a_head_is_an_error() {
        let e = select_head(URL, "").unwrap_err();
        assert_eq!(code(e), "refs::git::ref_not_found");
    }
}

mod check_version {
    use super::*;

    #[test]
    fn the_minimum_and_newer_pass() {
        for out in [
            "git version 2.36.0\n",
            "git version 2.55.0\n",
            "git version 3.0.1\n",
            "git version 2.39.5 (Apple Git-154)\n",
            "git version 2.43.0.windows.1\n",
            "git version 2.45.GIT\n",
        ] {
            assert!(check_version(out).is_ok(), "{out}");
        }
    }

    #[test]
    fn older_is_too_old() {
        for out in [
            "git version 2.35.9\n",
            "git version 2.34.1\n",
            "git version 1.9.0\n",
            "git version 2.35.GIT\n",
        ] {
            assert_eq!(
                code(check_version(out).unwrap_err()),
                "refs::git::too_old",
                "{out}"
            );
        }
    }

    #[test]
    fn output_that_is_not_a_version_is_an_error() {
        assert!(check_version("not git\n").is_err());
    }
}

mod check_input {
    use super::*;

    #[test]
    fn ordinary_input_passes() {
        assert!(check_input("https://github.com/o/r", "main").is_ok());
        assert!(check_input("git@github.com:o/r.git", "v1.0").is_ok());
    }

    #[test]
    fn option_like_input_is_rejected() {
        for (url, git_ref) in [
            ("--upload-pack=x", "main"),
            ("-oProxyCommand=x", "main"),
            ("https://example.com/o/r", "--upload-pack=x"),
            ("https://example.com/o/r", "-x"),
        ] {
            let e = check_input(url, git_ref).unwrap_err();
            assert_eq!(code(e), "refs::git::unsafe_input", "{url} {git_ref}");
        }
    }

    #[test]
    fn other_transports_are_rejected() {
        for url in ["ext::sh -c id", "fd::3", "http://example.com/o/r"] {
            let e = check_input(url, "main").unwrap_err();
            assert_eq!(code(e), "refs::git::unsafe_input", "{url}");
        }
    }
}

mod urls {
    use super::*;

    #[test]
    fn normalising_strips_the_tail_and_lowercases_the_host() {
        for (url, want) in [
            ("https://GitHub.com/O/R.git", "https://github.com/O/R"),
            ("https://github.com/o/r/", "https://github.com/o/r"),
            ("https://github.com/o/r.git/", "https://github.com/o/r"),
            (
                "ssh://Git@Example.COM:2222/o/r.git",
                "ssh://Git@example.com:2222/o/r",
            ),
            ("git@GitHub.com:o/r.git", "git@github.com:o/r"),
            ("file:///tmp/Repo.git", "file:///tmp/Repo"),
        ] {
            assert_eq!(normalise_url(url), want, "{url}");
        }
    }

    #[test]
    fn protocols_are_not_unified() {
        assert_ne!(
            normalise_url("https://github.com/o/r"),
            normalise_url("git@github.com:o/r")
        );
    }

    #[test]
    fn the_cache_name_is_16_hex_characters_of_the_normal_forms_sha256() {
        // printf 'https://github.com/o/r' | sha256sum
        assert_eq!(
            cache_dir_name("https://GitHub.com/o/r.git"),
            "97393e7e6b5ab8df"
        );
        assert_eq!(
            cache_dir_name("https://github.com/o/r"),
            cache_dir_name("https://github.com/o/r/")
        );
    }
}

mod cache_root {
    use super::*;
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
}
