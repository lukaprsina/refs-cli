//! The git tag that a package's version was released as.

/// The tag in `tags` that `name` at `version` was released as, if there is one. Exact matches
/// only, never a tag that merely ends in the version. Tags that name the package come before
/// the bare `v{version}` and `{version}`, since a monorepo's bare tag may belong to another
/// package. A scoped name (`@scope/pkg`) is tried in full before the part after the slash.
pub fn tag_for<'t>(name: &str, version: &str, tags: &'t [String]) -> Option<&'t str> {
    let short = name.rsplit_once('/').map(|(_, short)| short);
    let names = std::iter::once(name).chain(short.filter(|short| !short.is_empty()));
    let qualified = names.flat_map(|n| {
        [
            format!("{n}@{version}"),
            format!("{n}-v{version}"),
            format!("{n}-{version}"),
            format!("{n}_v{version}"),
            format!("{n}_{version}"),
        ]
    });
    qualified
        .chain([format!("v{version}"), version.to_owned()])
        .find_map(|candidate| tags.iter().find(|tag| **tag == candidate))
        .map(String::as_str)
}
