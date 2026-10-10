// Copyright 2025 Vercel Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file
// except in compliance with the License. You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software distributed under the
// License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
// either express or implied. See the License for the specific language governing permissions
// and limitations under the License.
//
// Modified for refs-cli: see the module comment below.

//! The small text helpers `pnpm-lock.yaml` and `yarn.lock` share: neither is parsed as YAML,
//! both are read line by line.
//!
//! Derived from the helpers in opensrc's `version.rs` (`packages/opensrc/cli/src/core/`,
//! Apache-2.0, see NOTICE). Unchanged apart from visibility.

/// Strip a pnpm peer-dependency suffix like `(react@18.0.0)` from a version string, so
/// `18.2.0(react@17.0.0)` becomes `18.2.0`. Cuts at the first `(` so nested peer suffixes like
/// `18.2.0(a@1)(b@2(c@3))` also collapse cleanly.
pub(super) fn strip_peer_suffix(v: &str) -> &str {
    match v.find('(') {
        Some(i) => v[..i].trim_end(),
        None => v.trim_end(),
    }
}

/// Strip a YAML-style inline comment. Only strips when `#` is preceded by whitespace, so URL
/// fragments like `github:foo/bar#branch` pass through intact.
fn strip_inline_comment(s: &str) -> &str {
    match s.find(" #") {
        Some(i) => s[..i].trim_end(),
        None => s,
    }
}

/// Strip any mix of surrounding single/double quotes from a trimmed string.
pub(super) fn trim_quotes(s: &str) -> &str {
    s.trim_matches(|c: char| c == '"' || c == '\'')
}

/// Normalise a raw YAML value: trim whitespace, strip an inline comment, and strip surrounding
/// quotes. Does NOT strip peer-dep suffixes: callers do that when appropriate.
pub(super) fn clean_value(s: &str) -> &str {
    let s = s.trim();
    let s = strip_inline_comment(s);
    trim_quotes(s)
}

/// Split a `<pkg>@<rest>` spec into `(name, rest)`, treating scoped names (`@scope/pkg`)
/// correctly. Returns `None` if there's no `@` separator.
pub(super) fn split_pkg_spec(spec: &str) -> Option<(&str, &str)> {
    let at_pos = if let Some(rest) = spec.strip_prefix('@') {
        rest.find('@').map(|i| i + 1)?
    } else {
        spec.find('@')?
    };
    Some((&spec[..at_pos], &spec[at_pos + 1..]))
}

/// Return `true` if `v` looks like a version we can resolve against a public registry.
/// Lockfiles can legitimately contain workspace/link/file/git/URL protocol strings: for example
/// a pnpm importer may pin a sibling workspace package with `version: link:../pkg`, and a yarn
/// Berry workspace root has `version: 0.0.0-use.local`. Real npm versions never contain `:`, so
/// treating a colon as disqualifying catches every known protocol prefix (`link:`, `file:`,
/// `workspace:`, `portal:`, `git:`, `git+ssh://`, `github:`, `http:`, `https:`, `npm:`, etc.)
/// without having to enumerate them.
pub(super) fn is_registry_version(v: &str) -> bool {
    !v.is_empty() && v != "0.0.0-use.local" && !v.contains(':')
}
