//! Tooling gaps (spec 7.6): tools whose config does not keep the references directory out.
//! refs never writes a tool's files; a gap says which lines to add.

use std::path::Path;

use crate::worktree::candidate_dirs;

/// A tool refs knows how to detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Prettier,
    Eslint,
    Oxlint,
    Tsc,
}

/// A detected tool whose files do not exclude the references directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub tool: Tool,
    /// The file to edit, as a path from the Project directory.
    pub file: String,
    /// The lines to add to it.
    pub snippet: String,
}

/// What `Tool::fix` gives: the file to edit and the lines to add.
type Fix = (String, String);

/// One tool: its name in `tooling_ignore`, the files that show it is in use in a directory (it
/// is covered when any of them names the references directory), and how to say what to add.
/// `fix` takes the files that are present and the references directory as seen from them.
/// Add a new tool as a row here and in `Tool`.
struct Row {
    tool: Tool,
    name: &'static str,
    files: &'static [&'static str],
    fix: fn(present: &[&str], dir: &str) -> Fix,
}

const TABLE: [Row; 4] = [
    Row {
        tool: Tool::Prettier,
        name: "prettier",
        files: &[
            ".prettierignore",
            ".prettierrc",
            ".prettierrc.json",
            ".prettierrc.yaml",
            ".prettierrc.yml",
            ".prettierrc.json5",
            ".prettierrc.toml",
            ".prettierrc.js",
            ".prettierrc.cjs",
            ".prettierrc.mjs",
            ".prettierrc.ts",
            "prettier.config.js",
            "prettier.config.cjs",
            "prettier.config.mjs",
            "prettier.config.ts",
        ],
        fix: |_, dir| (".prettierignore".into(), format!("{dir}/")),
    },
    Row {
        tool: Tool::Eslint,
        name: "eslint",
        files: &[
            "eslint.config.js",
            "eslint.config.mjs",
            "eslint.config.cjs",
            "eslint.config.ts",
            "eslint.config.mts",
            "eslint.config.cts",
            ".eslintrc",
            ".eslintrc.js",
            ".eslintrc.cjs",
            ".eslintrc.json",
            ".eslintrc.yaml",
            ".eslintrc.yml",
            ".eslintignore",
        ],
        fix: |present, dir| match present
            .iter()
            .find(|file| file.starts_with("eslint.config."))
        {
            Some(flat) => (flat.to_string(), format!(r#"{{ ignores: ["{dir}/**"] }}"#)),
            None => (".eslintignore".into(), format!("{dir}/")),
        },
    },
    Row {
        tool: Tool::Oxlint,
        name: "oxlint",
        files: &[".oxlintrc.json"],
        fix: |_, dir| {
            (
                ".oxlintrc.json".into(),
                format!(r#""ignorePatterns": ["{dir}/"]"#),
            )
        },
    },
    Row {
        tool: Tool::Tsc,
        name: "tsc",
        files: &["tsconfig.json"],
        fix: |_, dir| ("tsconfig.json".into(), format!(r#""exclude": ["{dir}"]"#)),
    },
];

impl Tool {
    /// The names `tooling_ignore` accepts.
    pub fn names() -> Vec<&'static str> {
        TABLE.iter().map(|row| row.name).collect()
    }

    /// The name `tooling_ignore` uses.
    pub fn name(self) -> &'static str {
        self.row().name
    }

    pub fn from_name(name: &str) -> Option<Tool> {
        TABLE
            .iter()
            .find(|row| row.name == name)
            .map(|row| row.tool)
    }

    fn row(self) -> &'static Row {
        TABLE.iter().find(|row| row.tool == self).expect("in TABLE")
    }
}

/// The tools in use for the project at `project_dir` whose files do not name `references_dir`,
/// except `ignored`. Each tool is read in the nearest directory that has a file of it, from
/// `project_dir` up to the top of its git worktree (`candidate_dirs`).
pub fn gaps(project_dir: &Path, references_dir: &str, ignored: &[Tool]) -> Vec<Gap> {
    let dirs = candidate_dirs(project_dir);
    let mut found = Vec::new();
    for row in TABLE.iter().filter(|row| !ignored.contains(&row.tool)) {
        let Some((up, dir, present)) = dirs.iter().enumerate().find_map(|(up, dir)| {
            let present: Vec<&str> = row
                .files
                .iter()
                .copied()
                .filter(|file| dir.join(file).is_file())
                .collect();
            (!present.is_empty()).then_some((up, dir, present))
        }) else {
            continue;
        };
        let covered = present.iter().any(|file| {
            std::fs::read_to_string(dir.join(file)).is_ok_and(|text| text.contains(references_dir))
        });
        if !covered {
            let (file, snippet) = (row.fix)(&present, &seen_from(dir, project_dir, references_dir));
            found.push(Gap {
                tool: row.tool,
                file: format!("{}{file}", "../".repeat(up)),
                snippet,
            });
        }
    }
    found
}

/// `references_dir` of the project at `project_dir`, as a `/`-separated path from `dir`, an
/// ancestor of it.
fn seen_from(dir: &Path, project_dir: &Path, references_dir: &str) -> String {
    let below = project_dir.strip_prefix(dir).unwrap_or(Path::new(""));
    let mut parts: Vec<String> = below
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.push(references_dir.to_owned());
    parts.join("/")
}
