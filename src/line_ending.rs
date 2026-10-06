//! The line ending a text uses, and giving other text the same.

/// `\r\n` if `text` has any, else `\n`.
pub(crate) fn of(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// `text` with every line ending replaced by `eol`.
pub(crate) fn convert(text: &str, eol: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', eol)
}
