//! Tooling gaps (spec 7.6): tools whose config does not keep the references directory out.
//! refs never writes a tool's files; a gap says which lines to add.

use std::path::Path;

use crate::worktree::candidate_dirs;

mod editors;
mod js;
mod python;
mod site;

use site::Site;

/// A detected tool whose files do not exclude the references directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// The tool's name, as `tooling_ignore` spells it.
    pub tool: &'static str,
    /// The file to edit, as a path from the Project directory.
    pub file: String,
    /// The lines to add to it.
    pub snippet: String,
    /// Whether `file` does not exist yet, so the note says to create it.
    pub create: bool,
}

impl Gap {
    /// What to do about it, in a sentence, and how to stop being told.
    pub fn advice(&self) -> String {
        let (file, snippet, tool) = (&self.file, &self.snippet, self.tool);
        let fix = match self.create {
            true => format!("create {file} with: {snippet}"),
            false => format!("add this to {file}: {snippet}"),
        };
        format!(r#"{fix} (or add "{tool}" to tooling_ignore in refs.toml)"#)
    }
}

/// What one tool finds in one directory.
enum Finding {
    /// The tool is not in use here.
    Absent,
    /// It is, and its files keep the references directory out.
    Covered,
    /// It is, and they do not: `file` (from the directory, it may not exist) needs `snippet`.
    Gap { file: String, snippet: String },
}

impl Finding {
    fn gap(file: &str, snippet: String) -> Finding {
        Finding::Gap {
            file: file.into(),
            snippet,
        }
    }
}

/// One tool's function.
type Check = fn(&Site) -> Finding;

/// The tools, by their name in `tooling_ignore`. Each says whether it is in use in a directory,
/// whether its files cover the references directory, and what to add if not. Add a new tool as
/// a function and a line here.
const TOOLS: &[(&str, Check)] = &[
    ("prettier", js::prettier),
    ("eslint", js::eslint),
    ("black", python::black),
    ("biome", js::biome),
    ("markdownlint", js::markdownlint),
    ("zed", editors::zed),
];

/// Names earlier versions reported. `tooling_ignore` still accepts them and they do nothing: the
/// tools skip the references directory by themselves (spec 7.6).
const RETIRED: [&str; 3] = ["oxlint", "tsc", "stylelint"];

/// The names `tooling_ignore` accepts.
pub fn names() -> Vec<&'static str> {
    TOOLS.iter().map(|&(name, _)| name).chain(RETIRED).collect()
}

/// The tools in use for the project at `project_dir` whose files do not name `references_dir`,
/// except `ignored`. Each tool is read in the nearest directory that shows it in use, from
/// `project_dir` up to the top of its git worktree (`candidate_dirs`).
pub fn gaps(project_dir: &Path, references_dir: &str, ignored: &[&str]) -> Vec<Gap> {
    let sites: Vec<Site> = candidate_dirs(project_dir)
        .into_iter()
        .map(|dir| Site::read(dir, project_dir, references_dir))
        .collect();
    let mut found = Vec::new();
    for &(tool, check) in TOOLS.iter().filter(|(name, _)| !ignored.contains(name)) {
        let nearest = sites
            .iter()
            .enumerate()
            .find_map(|(up, site)| match check(site) {
                Finding::Absent => None,
                finding => Some((up, site, finding)),
            });
        if let Some((up, site, Finding::Gap { file, snippet })) = nearest {
            found.push(Gap {
                tool,
                create: !site.dir.join(&file).is_file(),
                file: format!("{}{file}", "../".repeat(up)),
                snippet,
            });
        }
    }
    found
}
