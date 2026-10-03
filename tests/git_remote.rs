//! The pure half of git resolution, tested as data: no git is run.

use miette::Diagnostic;
use refs_cli::source::git::cache::cache_root;
use refs_cli::source::git::remote::{
    EntryKind, ancestor_dirs, cache_dir_name, check_input, check_version, commit_unavailable,
    entry_kind, missing_object, missing_oids, normalise_url, select_head, select_ref, tree_blobs,
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
    fn a_branch_resolves_to_its_commit() {
        let out = format!("{A}\trefs/heads/main\n");
        assert_eq!(select_ref(URL, "main", &out).unwrap(), A);
    }

    #[test]
    fn a_lightweight_tag_resolves_to_its_commit() {
        let out = format!("{A}\trefs/tags/v1\n");
        assert_eq!(select_ref(URL, "v1", &out).unwrap(), A);
    }

    #[test]
    fn an_annotated_tag_resolves_to_the_peeled_commit_not_the_tag_object() {
        let out = format!("{B}\trefs/tags/v1\n{A}\trefs/tags/v1^{{}}\n");
        assert_eq!(select_ref(URL, "v1", &out).unwrap(), A);
    }

    #[test]
    fn a_tag_wins_over_a_branch_of_the_same_name() {
        let out = format!("{B}\trefs/heads/v1\n{A}\trefs/tags/v1\n");
        assert_eq!(select_ref(URL, "v1", &out).unwrap(), A);
    }

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
    fn head_records_the_branch_it_points_at() {
        let out = format!("ref: refs/heads/trunk\tHEAD\n{A}\tHEAD\n{A}\trefs/heads/trunk\n");
        assert_eq!(
            select_head(URL, &out).unwrap(),
            (A.to_string(), Some("trunk".to_string()))
        );
    }

    #[test]
    fn a_detached_head_has_no_branch() {
        let out = format!("{A}\tHEAD\n");
        assert_eq!(select_head(URL, &out).unwrap(), (A.to_string(), None));
    }

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

mod tree_blobs {
    use super::*;

    #[test]
    fn lists_blob_ids_once_in_order_and_skips_trees_and_submodules() {
        let out = format!(
            "100644 blob {A}\tREADME.md\n\
             040000 tree {B}\tdocs\n\
             160000 commit {C}\tvendor\n\
             100755 blob {B}\tbin/run\n\
             100644 blob {A}\tcopy.md\n"
        );
        assert_eq!(tree_blobs(&out), [A, B]);
    }

    #[test]
    fn no_output_is_no_blobs() {
        assert!(tree_blobs("").is_empty());
    }
}

mod entry_kind {
    use super::*;

    #[test]
    fn a_tree_a_file_and_nothing() {
        assert_eq!(
            entry_kind(&format!("040000 tree {A}\tdocs\n")),
            EntryKind::Tree
        );
        assert_eq!(
            entry_kind(&format!("100644 blob {A}\tREADME.md\n")),
            EntryKind::Other
        );
        assert_eq!(entry_kind(""), EntryKind::Missing);
    }
}

mod fetch_failures {
    use super::*;

    #[test]
    fn a_refused_commit_is_recognised() {
        assert!(commit_unavailable(
            "fatal: remote error: upload-pack: not our ref aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        assert!(commit_unavailable(
            "error: Server does not allow request for unadvertised object aaaa"
        ));
    }

    #[test]
    fn other_failures_are_not_a_refused_commit() {
        assert!(!commit_unavailable(
            "fatal: unable to access 'https://x/': Could not resolve host"
        ));
    }

    #[test]
    fn the_missing_object_is_named_from_a_promisor_failure() {
        let stderr = format!("fatal: could not fetch {A} from promisor remote\n");
        assert_eq!(missing_object(&stderr), Some(A.to_string()));
    }

    #[test]
    fn another_failure_names_no_object() {
        assert_eq!(missing_object("fatal: not a git repository"), None);
    }
}

mod ancestor_dirs {
    use super::*;

    fn dirs(paths: &[&str]) -> Vec<String> {
        ancestor_dirs(&paths.iter().map(|p| p.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_top_level_path_has_no_ancestors() {
        assert!(dirs(&["docs"]).is_empty());
    }

    #[test]
    fn every_directory_above_a_path_counts_once() {
        assert_eq!(
            dirs(&["docs/guide/sub", "docs/api", "src"]),
            ["docs/", "docs/guide/"]
        );
    }
}

mod missing_oids {
    use super::*;

    #[test]
    fn only_the_missing_ones() {
        let out = format!("{A} missing\n{B} blob 12\n{C} missing\n");
        assert_eq!(missing_oids(&out), [A, C]);
    }
}

mod cache_root {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn xdg_cache_home_wins() {
        assert_eq!(
            cache_root(Some("/x/cache"), Some("/home/u")),
            Some(PathBuf::from("/x/cache/refs"))
        );
    }

    #[test]
    fn the_default_is_dot_cache_in_home() {
        assert_eq!(
            cache_root(None, Some("/home/u")),
            Some(PathBuf::from("/home/u/.cache/refs"))
        );
    }

    #[test]
    fn a_relative_xdg_cache_home_is_ignored() {
        assert_eq!(
            cache_root(Some("cache"), Some("/home/u")),
            Some(PathBuf::from("/home/u/.cache/refs"))
        );
    }

    #[test]
    fn with_neither_there_is_no_cache() {
        assert_eq!(cache_root(None, None), None);
    }
}
