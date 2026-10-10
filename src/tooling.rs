//! Tooling gaps (spec 7.5): tools whose config does not keep the references directory out.
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

/// A detected tool whose config does not exclude the references directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub tool: Tool,
    /// The file to edit, as a path from the Project directory.
    pub config: String,
    /// The lines to add to it.
    pub snippet: String,
}

/// The files that show a tool is in use in a directory. A tool is covered when any of them
/// names the references directory. Add a new tool here and to `Tool::fix`.
const TABLE: [(Tool, &str, &[&str]); 4] = [
    (
        Tool::Prettier,
        "prettier",
        &[
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
    ),
    (
        Tool::Eslint,
        "eslint",
        &[
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
    ),
    (Tool::Oxlint, "oxlint", &[".oxlintrc.json"]),
    (Tool::Tsc, "tsc", &["tsconfig.json"]),
];

impl Tool {
    /// The names `tooling_ignore` accepts.
    pub fn names() -> Vec<&'static str> {
        TABLE.iter().map(|row| row.1).collect()
    }

    /// The name `tooling_ignore` uses.
    pub fn name(self) -> &'static str {
        self.row().1
    }

    pub fn from_name(name: &str) -> Option<Tool> {
        TABLE
            .iter()
            .find_map(|&(tool, known, _)| (known == name).then_some(tool))
    }

    fn row(self) -> &'static (Tool, &'static str, &'static [&'static str]) {
        TABLE.iter().find(|row| row.0 == self).expect("in TABLE")
    }

    /// The file to edit and the lines to add, given the files of the tool that are present and
    /// the references directory as seen from their directory.
    fn fix(self, present: &[&str], dir: &str) -> (String, String) {
        match self {
            Tool::Prettier => (".prettierignore".into(), format!("{dir}/")),
            Tool::Eslint => match present
                .iter()
                .find(|file| file.starts_with("eslint.config."))
            {
                Some(flat) => (flat.to_string(), format!(r#"{{ ignores: ["{dir}/**"] }}"#)),
                None => (".eslintignore".into(), format!("{dir}/")),
            },
            Tool::Oxlint => (
                ".oxlintrc.json".into(),
                format!(r#""ignorePatterns": ["{dir}/"]"#),
            ),
            Tool::Tsc => ("tsconfig.json".into(), format!(r#""exclude": ["{dir}"]"#)),
        }
    }
}

/// The tools in use for the project at `project_dir` whose config does not name
/// `references_dir`, except `ignored`. Each tool is read in the nearest directory that has a
/// file of it, from `project_dir` up to the top of its git worktree (`candidate_dirs`).
pub fn gaps(project_dir: &Path, references_dir: &str, ignored: &[Tool]) -> Vec<Gap> {
    let dirs = candidate_dirs(project_dir);
    let mut found = Vec::new();
    for &(tool, _, files) in &TABLE {
        if ignored.contains(&tool) {
            continue;
        }
        for (up, dir) in dirs.iter().enumerate() {
            let present: Vec<&str> = files
                .iter()
                .copied()
                .filter(|file| dir.join(file).is_file())
                .collect();
            if present.is_empty() {
                continue;
            }
            let covered = present.iter().any(|file| {
                std::fs::read_to_string(dir.join(file))
                    .is_ok_and(|text| text.contains(references_dir))
            });
            if !covered {
                let below = project_dir.strip_prefix(dir).unwrap_or(Path::new(""));
                let mut dir_from_config: Vec<String> = below
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                dir_from_config.push(references_dir.to_owned());
                let (file, snippet) = tool.fix(&present, &dir_from_config.join("/"));
                found.push(Gap {
                    tool,
                    config: format!("{}{file}", "../".repeat(up)),
                    snippet,
                });
            }
            break;
        }
    }
    found
}
