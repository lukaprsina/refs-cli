//! The pure core of the Exclude rule: file text in, answer or file text out. No git.

use refs_cli::exclude::{rule_present, with_rule};

const RULE: &str = "/refs/";

#[test]
fn presence_table() {
    let cases = [
        ("the rule alone", "/refs/\n", true),
        ("the rule among others", "target/\n/refs/\n*.log\n", true),
        ("absent", "target/\n*.log\n", false),
        ("another directory's rule", "/other/\n", false),
        ("the same name anchored elsewhere", "/sub/refs/\n", false),
        ("a trailing-whitespace line", "/refs/  \n", true),
        ("no final newline", "target/\n/refs/", true),
        ("empty file", "", false),
    ];
    for (name, text, expected) in cases {
        assert_eq!(rule_present(text, RULE), expected, "{name}");
    }
}

#[test]
fn appending_table() {
    let cases = [
        ("empty file", "", "/refs/\n"),
        ("final newline", "target/\n", "target/\n/refs/\n"),
        ("no final newline", "target/", "target/\n/refs/\n"),
    ];
    for (name, text, expected) in cases {
        assert_eq!(with_rule(text, RULE), expected, "{name}");
    }
}

#[test]
fn appended_rule_is_then_present() {
    for text in ["", "a\n", "a"] {
        assert!(rule_present(&with_rule(text, RULE), RULE), "{text:?}");
    }
}
