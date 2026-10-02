use refs_cli::config::parse;

#[test]
fn repos_keep_file_order() {
    let text = r#"
[repos.zeta]
url = "https://github.com/o/zeta"

[repos.alpha]
url = "https://github.com/o/alpha"
"#;
    let config = parse(text).unwrap();
    let ids: Vec<&str> = config.repos.keys().map(|id| id.as_ref().as_str()).collect();
    assert_eq!(ids, ["zeta", "alpha"]);
}

#[test]
fn settings_table_is_accepted() {
    let text = r#"
[settings]
references_dir = ".refs"
agents_files = ["AGENTS.md", "CLAUDE.md"]
"#;
    parse(text).unwrap();
}

mod diagnostics {
    use miette::Diagnostic;
    use refs_cli::config::parse;

    /// `(code, text under the first label)` for every error, in report order.
    pub fn report(text: &str) -> Vec<(String, Option<String>)> {
        let errors = parse(text).unwrap_err();
        errors
            .errors
            .iter()
            .map(|e| {
                let code = e.code().unwrap().to_string();
                let span = e
                    .labels()
                    .and_then(|mut l| l.next())
                    .map(|l| text[l.offset()..l.offset() + l.len()].to_string());
                (code, span)
            })
            .collect()
    }
}

#[test]
fn unknown_key_fails_fast_with_a_span_on_the_key() {
    let text = "[repos.a]\nurl = \"https://github.com/o/a\"\nbogus = 1\n";
    let report = diagnostics::report(text);
    assert_eq!(
        report,
        [(
            "refs::config::unknown_key".to_string(),
            Some("bogus".to_string())
        )]
    );
}

fn repo(extra: &str) -> String {
    format!("[repos.a]\nurl = \"https://github.com/o/a\"\n{extra}")
}

fn code_and_span(text: &str, code: &str, span: &str) {
    let report = diagnostics::report(text);
    assert_eq!(
        report,
        [(code.to_string(), Some(span.to_string()))],
        "{text}"
    );
}

#[test]
fn repo_id_must_be_a_safe_directory_name() {
    for bad in ["Bad", "-x", "_a", "A1"] {
        let text = format!("[repos.{bad}]\nurl = \"https://github.com/o/a\"\n");
        code_and_span(&text, "refs::config::bad_id", bad);
    }
    parse(&repo("")).unwrap();
    parse("[repos.a1-b_c]\nurl = \"https://github.com/o/a\"\n").unwrap();
}

#[test]
fn repo_group_must_exist() {
    let text = repo("group = \"nope\"\n");
    code_and_span(&text, "refs::config::dangling_group", "\"nope\"");
    let ok = format!("[groups.g]\nname = \"G\"\n{}", repo("group = \"g\"\n"));
    parse(&ok).unwrap();
}

#[test]
fn start_is_inside_paths_or_a_root_file() {
    let with = |start: &str| {
        repo(&format!(
            "paths = [\"docs/guide\"]\nstart = [\"{start}\"]\n"
        ))
    };
    parse(&with("README.md")).unwrap();
    parse(&with("docs/guide/a.md")).unwrap();
    for bad in ["docs/README.md", "src/x.rs"] {
        code_and_span(
            &with(bad),
            "refs::config::start_outside_paths",
            &format!("\"{bad}\""),
        );
    }
    // without `paths` the whole repo is checked out, so anything goes
    parse(&repo("start = [\"src/x.rs\"]\n")).unwrap();
}

#[test]
fn paths_and_start_are_relative_without_dot_dot() {
    for bad in ["../x", "/abs", "a/../b"] {
        code_and_span(
            &repo(&format!("paths = [\"{bad}\"]\n")),
            "refs::config::bad_path",
            &format!("\"{bad}\""),
        );
    }
    let text = repo("paths = [\"docs/guide\"]\nstart = [\"docs/guide/../x.md\"]\n");
    code_and_span(&text, "refs::config::bad_path", "\"docs/guide/../x.md\"");
}

#[test]
fn url_must_not_look_like_an_option_or_carry_a_password() {
    let with = |url: &str| format!("[repos.a]\nurl = \"{url}\"\n");
    for bad in [
        "-oProxyCommand=x",
        "https://user:token@github.com/o/a",
        "ssh://u:p@host/o/a",
    ] {
        code_and_span(&with(bad), "refs::config::bad_url", &format!("\"{bad}\""));
    }
    for ok in [
        "git@github.com:o/a",
        "https://user@github.com/o/a",
        "file:///tmp/a",
        "git://host/a",
    ] {
        parse(&with(ok)).unwrap();
    }
}

#[test]
fn url_must_use_an_allowed_transport() {
    let with = |url: &str| format!("[repos.a]\nurl = \"{url}\"\n");
    for bad in [
        "ext::sh -c id@host:x",
        "fd::3",
        "ftp://host/a",
        "http://github.com/o/a",
        "github.com/o/a",
        "word",
        "/tmp/a",
        "host:path",
        "@host:path",
        "user@:path",
        "user@ho/st:path",
        "git@-oProxyCommand=x:p",
    ] {
        code_and_span(&with(bad), "refs::config::bad_url", &format!("\"{bad}\""));
    }
    for ok in [
        "https://github.com/o/a",
        "ssh://git@host/o/a",
        "git://host/a",
        "file:///tmp/a",
        "git@github.com:owner/repo.git",
    ] {
        parse(&with(ok)).unwrap();
    }
}

#[test]
fn rejected_transports_get_the_transport_message_not_the_password_one() {
    for bad in ["ext::sh -c id@host:x", "fd::3", "/tmp/x@y:z", "../a@b:c"] {
        let text = format!("[repos.a]\nurl = \"{bad}\"\n");
        let err = refs_cli::config::parse(&text).unwrap_err();
        assert!(
            format!("{:?}", err.errors[0]).contains("must be https, ssh, git or file"),
            "{bad}: {err:?}"
        );
    }
}

#[test]
fn scp_style_path_may_contain_a_scheme_separator() {
    parse("[repos.a]\nurl = \"git@host:x://y\"\n").unwrap();
}

#[test]
fn scp_style_url_must_not_carry_a_password() {
    let text = "[repos.a]\nurl = \"user:pass@host:o/a\"\n";
    code_and_span(text, "refs::config::bad_url", "\"user:pass@host:o/a\"");
}

#[test]
fn ref_must_not_look_like_an_option() {
    code_and_span(
        &repo("ref = \"--upload-pack=x\"\n"),
        "refs::config::bad_ref",
        "\"--upload-pack=x\"",
    );
    parse(&repo("ref = \"main\"\n")).unwrap();
}

#[test]
fn group_name_is_heading_safe() {
    let group = |name: &str| format!("[groups.g]\nname = \"{name}\"\n");
    for bad in ["Bad#", " lead", "trail ", "a\\nb", "*x*"] {
        code_and_span(
            &group(bad),
            "refs::config::bad_group_name",
            &format!("\"{bad}\""),
        );
    }
    for ok in [
        "SolidJS 2.0",
        "Rust (std) + C/C++ & more, v1: x-y",
        "Ünïcode 7",
    ] {
        parse(&group(ok)).unwrap();
    }
}

#[test]
fn rendered_strings_are_single_line_and_cannot_close_the_block() {
    let cases = [
        ("description", "a\\nb"),
        ("description", "tab\\there"),
        ("description", "```"),
        ("description", "x BEGIN:refs y"),
        ("description", "END:refs"),
    ];
    for (key, bad) in cases {
        code_and_span(
            &repo(&format!("{key} = \"{bad}\"\n")),
            "refs::config::unsafe_text",
            &format!("\"{bad}\""),
        );
    }
    code_and_span(
        &repo("packages = [\"ok\", \"a\\nb\"]\n"),
        "refs::config::unsafe_text",
        "\"a\\nb\"",
    );
    code_and_span(
        &repo("start = [\"a\\nb.md\"]\n"),
        "refs::config::unsafe_text",
        "\"a\\nb.md\"",
    );
    let group = "[groups.g]\nname = \"G\"\ndescription = \"x\\r\"\n";
    code_and_span(group, "refs::config::unsafe_text", "\"x\\r\"");
    parse(&repo("description = \"fine, with `code` and ``\"\n")).unwrap();
}

#[test]
fn semantic_errors_are_reported_together() {
    let text = "[repos.Bad]\nurl = \"-x\"\ngroup = \"nope\"\nref = \"-y\"\n";
    let codes: Vec<String> = diagnostics::report(text)
        .into_iter()
        .map(|(code, _)| code)
        .collect();
    for expected in ["bad_id", "bad_url", "bad_ref", "dangling_group"] {
        assert!(
            codes.contains(&format!("refs::config::{expected}")),
            "{expected} in {codes:?}"
        );
    }
    assert_eq!(codes.len(), 4);
}

#[test]
fn rendered_report_points_at_the_offending_text() {
    use miette::{GraphicalReportHandler, GraphicalTheme};
    let text = "[repos.a]\nurl = \"https://me:secret@github.com/o/a\"\n";
    let errors = parse(text).unwrap_err();
    let handler = GraphicalReportHandler::new_themed(GraphicalTheme::none());
    let mut out = String::new();
    for error in &errors.errors {
        handler.render_report(&mut out, error).unwrap();
    }
    insta::assert_snapshot!(out);
}

#[test]
fn group_name_cannot_carry_marker_text() {
    for bad in ["BEGIN:refs", "x END:refs"] {
        let text = format!("[groups.g]\nname = \"{bad}\"\n");
        code_and_span(&text, "refs::config::bad_group_name", &format!("\"{bad}\""));
    }
}

#[test]
fn disabled_entries_are_still_validated() {
    code_and_span(
        &repo("enabled = false\ngroup = \"nope\"\n"),
        "refs::config::dangling_group",
        "\"nope\"",
    );
}

#[test]
fn unknown_keys_are_rejected_in_every_table() {
    for text in [
        "bogus = 1\n",
        "[settings]\nbogus = 1\n",
        "[groups.g]\nname = \"G\"\nbogus = 1\n",
    ] {
        code_and_span(text, "refs::config::unknown_key", "bogus");
    }
}
