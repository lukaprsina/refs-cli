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

//! `pnpm-lock.yaml`, read line by line with an indent-aware stack. A version an importer (or,
//! in v5/v6 lockfiles, the top level) lists is one the project asks for: `direct`. Every other
//! version in `packages:` or `snapshots:` is something the project's packages use.
//!
//! Derived from opensrc's `parse_pnpm_lock` (`packages/opensrc/cli/src/core/version.rs`,
//! Apache-2.0, see NOTICE). Changed: it returns every version of the package, each marked
//! direct or not, instead of the first by priority; the dependency graph and the
//! breadth-first search over it are gone, as a version only reachable through another package
//! is simply not direct; and the tests moved to `tests/registry_used.rs`.

use super::Used;
use super::yaml::{
    clean_value, is_registry_version, split_pkg_spec, strip_peer_suffix, trim_quotes,
};

/// A frame on the indent-aware parse stack. The `usize` is the indent of the line that opened
/// the frame; children must be at indent strictly greater than that value. `Frame::Root` has
/// no header line and is never popped.
#[derive(Clone, Debug)]
enum Frame {
    Root,
    Importers(usize),
    Importer(usize),
    DepGroup(usize),
    /// Block-form dep entry awaiting a nested `version:` line.
    DepBlock {
        base: usize,
        pkg_name: String,
    },
    Packages(usize),
    Snapshots(usize),
    /// Inside a `packages:` or `snapshots:` entry, whose lines are not read.
    PkgEntry(usize),
}

impl Frame {
    fn base(&self) -> Option<usize> {
        match self {
            Frame::Root => None,
            Frame::Importers(b)
            | Frame::Importer(b)
            | Frame::DepGroup(b)
            | Frame::Packages(b)
            | Frame::Snapshots(b)
            | Frame::PkgEntry(b) => Some(*b),
            Frame::DepBlock { base, .. } => Some(*base),
        }
    }
}

const DEP_GROUPS: [&str; 3] = ["dependencies", "devDependencies", "optionalDependencies"];

pub(super) fn used(pkg: &str, text: &str) -> Vec<Used> {
    let mut stack: Vec<Frame> = vec![Frame::Root];
    let mut found: Vec<Used> = Vec::new();
    let ask = |found: &mut Vec<Used>, name: &str, value: &str, direct: bool| {
        // Filter at capture so workspace/link/file versions in one importer don't block a real
        // version in a later importer.
        let version = strip_peer_suffix(value);
        if name == pkg && is_registry_version(version) {
            found.push(Used {
                version: version.to_owned(),
                direct,
            });
        }
    };

    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let content = &line[indent..];

        // Pop frames whose scope has ended. Root (base == None) never pops.
        while let Some(base) = stack.last().and_then(Frame::base) {
            if indent > base {
                break;
            }
            stack.pop();
        }

        let top = stack.last().expect("stack never empty").clone();
        match top {
            Frame::Root => {
                if indent == 0
                    && let Some(key) = content.strip_suffix(':')
                {
                    match key.trim() {
                        "importers" => stack.push(Frame::Importers(indent)),
                        key if DEP_GROUPS.contains(&key) => {
                            stack.push(Frame::DepGroup(indent));
                        }
                        "packages" => stack.push(Frame::Packages(indent)),
                        "snapshots" => stack.push(Frame::Snapshots(indent)),
                        _ => {}
                    }
                }
            }
            Frame::Importers(_) => {
                if content.ends_with(':') {
                    stack.push(Frame::Importer(indent));
                }
            }
            Frame::Importer(_) => {
                if let Some(key) = content.strip_suffix(':')
                    && DEP_GROUPS.contains(&key.trim())
                {
                    stack.push(Frame::DepGroup(indent));
                }
            }
            Frame::DepGroup(_) => {
                if let Some((k, v)) = content.split_once(':') {
                    let dep_name = trim_quotes(k.trim()).to_string();
                    let raw_value = v.trim();
                    if raw_value.is_empty() {
                        // Block form: version comes on a nested line.
                        stack.push(Frame::DepBlock {
                            base: indent,
                            pkg_name: dep_name,
                        });
                    } else {
                        ask(&mut found, &dep_name, clean_value(raw_value), true);
                    }
                }
            }
            Frame::DepBlock { pkg_name, .. } => {
                if let Some(rest) = content.strip_prefix("version:") {
                    ask(&mut found, &pkg_name, clean_value(rest), true);
                    stack.pop();
                }
            }
            Frame::Packages(_) | Frame::Snapshots(_) => {
                if let Some((key_part, value_part)) = content.split_once(':') {
                    let key = trim_quotes(key_part.trim());
                    let key = key.strip_prefix('/').unwrap_or(key);
                    if let Some((name, version)) = split_pkg_spec(key) {
                        ask(&mut found, name, version, false);
                        if value_part.trim().is_empty() {
                            stack.push(Frame::PkgEntry(indent));
                        }
                        // Else: inline value like `{}`, no children to skip.
                    }
                }
            }
            Frame::PkgEntry(_) => {}
        }
    }
    found
}
