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

/// `tag` as a prefix and the semver version after it: the version starts at the first position
/// where the rest of the tag parses, so `foo2-1.0.0` is `foo2-` and `1.0.0`.
pub fn split(tag: &str) -> Option<(&str, semver::Version)> {
    tag.char_indices().find_map(|(at, _)| {
        let version = semver::Version::parse(&tag[at..]).ok()?;
        Some((&tag[..at], version))
    })
}

/// The tag in `tags` with the highest version among those with the same prefix as `current`
/// (equal, not "starts with": `react-` is not `react-dom-`), so a sibling Package of a monorepo
/// is never chosen. A stable `current` ignores prereleases. None when `current` has no version.
pub fn newest<'t>(current: &str, tags: &'t [String]) -> Option<&'t str> {
    let (prefix, version) = split(current)?;
    let stable = version.pre.is_empty();
    tags.iter()
        .filter_map(|tag| Some((tag.as_str(), split(tag)?)))
        .filter(|(_, (p, v))| *p == prefix && (!stable || v.pre.is_empty()))
        .max_by(|a, b| a.1.1.cmp(&b.1.1))
        .map(|(tag, _)| tag)
}
