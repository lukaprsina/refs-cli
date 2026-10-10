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

//! `yarn.lock`, classic (v1) or Berry (v2+): blank-line separated blocks, each a header of
//! comma-separated `name@range` specs and a `version` line. The format does not say which
//! packages the project asks for, so no version is direct.
//!
//! Derived from opensrc's `parse_yarn_lock` (`packages/opensrc/cli/src/core/version.rs`,
//! Apache-2.0, see NOTICE). Changed: it returns the version of every block that names the
//! package, not only the first, finds an alias by the package it stands for, and the tests
//! moved to `tests/registry_used.rs`.

use super::Used;
use super::yaml::{
    clean_value, is_registry_version, split_pkg_spec, strip_peer_suffix, trim_quotes,
};

/// The package a spec asks for. For an alias (`short@npm:@scope/long@^2`, Berry) that is the
/// package after `npm:`, not the name it is installed under.
fn real_name<'a>(name: &'a str, range: &'a str) -> &'a str {
    range
        .strip_prefix("npm:")
        .and_then(split_pkg_spec)
        .map_or(name, |(real, _)| real)
}

pub(super) fn used(pkg: &str, text: &str) -> Vec<Used> {
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }

    let mut found = Vec::new();
    for block in &blocks {
        let mut header: Option<&str> = None;
        let mut body: Vec<&str> = Vec::with_capacity(block.len());

        for &line in block {
            if line.trim_start().starts_with('#') {
                continue;
            }
            if header.is_none() && !line.starts_with(char::is_whitespace) {
                header = Some(line);
            } else {
                body.push(line);
            }
        }

        let Some(header) = header else { continue };
        if header.starts_with("__metadata:") {
            continue;
        }
        let Some(header_body) = header.strip_suffix(':') else {
            continue;
        };

        // Splitting on `, ` covers both:
        //   v1:    "foo@^1.0.0", "foo@~1.2.0":
        //   Berry: "foo@npm:^1.0.0, foo@workspace:*":
        // In the Berry case, the first split part keeps a leading `"` and the last keeps a
        // trailing `"`; `trim_quotes` strips either form.
        let matched = header_body.split(", ").any(|s| {
            let spec = trim_quotes(s.trim());
            split_pkg_spec(spec).is_some_and(|(name, range)| real_name(name, range) == pkg)
        });
        if !matched {
            continue;
        }

        for line in &body {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix("version") else {
                continue;
            };
            // Must be followed by `:` (Berry) or whitespace (v1) to be the version key, not
            // e.g. `versions:`.
            if !matches!(rest.chars().next(), Some(':' | ' ' | '\t')) {
                continue;
            }
            let rest = rest.trim_start();
            let rest = rest.strip_prefix(':').unwrap_or(rest);
            let version = strip_peer_suffix(clean_value(rest));
            // Skip workspace sentinels (`0.0.0-use.local`) and protocol strings
            // (`workspace:.`, `portal:.`, etc.): they cannot be looked up in a registry. A
            // real block for the same name elsewhere in the file is found on its own.
            if is_registry_version(version) {
                found.push(Used {
                    version: version.to_owned(),
                    direct: false,
                });
                break;
            }
        }
    }
    found
}
