use refs_cli::agent_file::splice;

const BEGIN: &str = "<!-- BEGIN:refs -->";
const END: &str = "<!-- END:refs -->";

/// A whole block, markers included, the way `render` produces it.
fn block(inner: &str) -> String {
    format!("{BEGIN}\n\n{inner}\n\n{END}")
}

#[test]
fn the_block_replaces_what_is_between_the_markers_and_nothing_else() {
    let old = format!("# Notes\n\nbefore\n{}\nafter\n", block("old"));
    let new = splice(&old, &block("new")).unwrap();
    assert_eq!(new, format!("# Notes\n\nbefore\n{}\nafter\n", block("new")));
}

#[test]
fn splicing_the_block_that_is_already_there_changes_nothing() {
    for text in [
        format!("a\n{}\nb\n", block("x")),
        block("x"),
        format!("a\n{}", block("x")),
        format!("a\r\n{}\r\nb\r\n", block("x").replace('\n', "\r\n")),
    ] {
        assert_eq!(splice(&text, &block("x")).unwrap(), text);
    }
}

#[test]
fn missing_markers_append_the_block_after_a_blank_line() {
    assert_eq!(
        splice("", &block("x")).unwrap(),
        format!("{}\n", block("x"))
    );
    assert_eq!(
        splice("# Notes\n", &block("x")).unwrap(),
        format!("# Notes\n\n{}\n", block("x"))
    );
    assert_eq!(
        splice("# Notes", &block("x")).unwrap(),
        format!("# Notes\n\n{}\n", block("x"))
    );
}

#[test]
fn an_appended_block_is_stable() {
    let once = splice("# Notes\n", &block("x")).unwrap();
    assert_eq!(splice(&once, &block("x")).unwrap(), once);
}

#[test]
fn a_crlf_file_gets_a_crlf_block_and_keeps_its_other_bytes() {
    let old = format!("a\r\n{}\r\nb", block("old").replace('\n', "\r\n"));
    let new = splice(&old, &block("new")).unwrap();
    assert_eq!(
        new,
        format!("a\r\n{}\r\nb", block("new").replace('\n', "\r\n"))
    );
    assert_eq!(
        splice("a\r\n", &block("x")).unwrap(),
        format!("a\r\n\r\n{}\r\n", block("x").replace('\n', "\r\n"))
    );
}

#[test]
fn a_marker_mentioned_in_prose_is_not_a_marker() {
    let text = format!("Use `{BEGIN}` and `{END}` to mark it.\n");
    let spliced = splice(&text, &block("x")).unwrap();
    assert!(spliced.starts_with(&text) && spliced.ends_with(&format!("{}\n", block("x"))));
}

fn refusal(text: &str) -> String {
    use miette::Diagnostic;
    let err = splice(text, &block("x")).unwrap_err();
    err.code().unwrap().to_string()
}

#[test]
fn malformed_markers_are_refused_with_a_code_each() {
    let cases = [
        (format!("a\n{BEGIN}\nb\n"), "unbalanced"),
        (format!("a\n{END}\nb\n"), "unbalanced"),
        (format!("{BEGIN}\n{BEGIN}\n{END}\n{END}\n"), "nested"),
        (format!("{BEGIN}\n{BEGIN}\n{END}\n"), "nested"),
        (format!("{END}\nx\n{BEGIN}\n"), "reversed"),
        (
            format!("{BEGIN}\nx\n{END}\n{BEGIN}\nx\n{END}\n"),
            "duplicated",
        ),
        (format!("{BEGIN}\nx\n{END}\n{END}\n"), "duplicated"),
    ];
    for (text, code) in cases {
        assert_eq!(refusal(&text), format!("refs::block::{code}"), "{text:?}");
    }
}

mod write {
    use miette::Diagnostic;
    use refs_cli::agent_file::write;

    #[test]
    fn creates_and_replaces_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AGENTS.md");
        write(&path, "one\n").unwrap();
        write(&path, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "no temp file left"
        );
    }

    #[cfg(unix)]
    #[test]
    fn writing_through_a_symlink_keeps_the_link_and_the_mode() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("AGENTS.md");
        let link = dir.path().join("CLAUDE.md");
        std::fs::write(&real, "old").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        symlink("AGENTS.md", &link).unwrap();

        write(&link, "new").unwrap();

        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new");
        let mode = std::fs::metadata(&real).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }

    #[test]
    fn a_failed_write_is_reported_with_a_code() {
        let dir = tempfile::tempdir().unwrap();
        let err = write(&dir.path().join("missing/AGENTS.md"), "x").unwrap_err();
        assert_eq!(err.code().unwrap().to_string(), "refs::block::write_failed");
    }
}

#[test]
fn a_file_with_one_stray_crlf_still_round_trips_its_lf_block() {
    let text = format!("a\r\nb\n{}\nc\n", block("x"));
    assert_eq!(splice(&text, &block("x")).unwrap(), text);
}
