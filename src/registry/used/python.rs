//! What `uv.lock` and `poetry.lock` share: which project names are the same, and which package
//! index counts as PyPI.

/// The name of a project with case and separators folded (PEP 503), so `Pillow`, `pillow` and
/// `Typing_Extensions` are found by `pillow` and `typing-extensions`.
fn normalized(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

/// Whether `a` and `b` name the same project.
pub(super) fn same_project(a: &str, b: &str) -> bool {
    normalized(a) == normalized(b)
}

/// Whether `url` is PyPI's own index. Another index may serve a different project under the
/// same name, and `pypi:` asks pypi.org where the project lives, so a version from there is not
/// a version of that project.
pub(super) fn is_pypi_index(url: &str) -> bool {
    url.trim_end_matches('/') == "https://pypi.org/simple"
}
