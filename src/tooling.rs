//! Tooling gaps (spec 7.6): tools whose config does not keep the references directory out.
//! refs never writes a tool's files; a gap says which lines to add.

use std::path::{Path, PathBuf};

use crate::worktree::candidate_dirs;

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

/// What `Row::fix` gives: the file to edit and the lines to add.
type Fix = (String, String);

/// What besides its own files shows a tool is in use: a key or entry in a manifest of the
/// directory, or a hook in its pre-commit config. Nothing is read from lockfiles (transitive
/// dependencies) or from the Python manifests but the `[tool.*]` tables.
enum Signal {
    /// A top-level key of `package.json` (`"prettier"`).
    PackageKey(&'static str),
    /// A direct entry in `dependencies` or `devDependencies` of `package.json`.
    Dependency(&'static str),
    /// A table `[tool.<name>]` of `pyproject.toml`.
    PyprojectTable(&'static str),
    /// A hook whose id starts with this, in `.pre-commit-config.yaml`.
    PreCommitHook(&'static str),
}

const PACKAGE_JSON: &str = "package.json";
const PYPROJECT: &str = "pyproject.toml";
const PRE_COMMIT: &str = ".pre-commit-config.yaml";

/// The manifests of one directory, read once for every row.
#[derive(Default)]
struct Facts {
    package: Option<serde_json::Value>,
    pyproject: Option<toml::Table>,
    pre_commit: Option<String>,
}

impl Facts {
    /// What is in `dir`. A file that cannot be read or parsed says nothing.
    fn read(dir: &Path) -> Facts {
        let text = |file: &str| std::fs::read_to_string(dir.join(file)).ok();
        Facts {
            package: text(PACKAGE_JSON).and_then(|text| serde_json::from_str(&text).ok()),
            pyproject: text(PYPROJECT).and_then(|text| text.parse().ok()),
            pre_commit: text(PRE_COMMIT),
        }
    }
}

impl Signal {
    /// The file the signal is in.
    fn file(&self) -> &'static str {
        match self {
            Signal::PackageKey(_) | Signal::Dependency(_) => PACKAGE_JSON,
            Signal::PyprojectTable(_) => PYPROJECT,
            Signal::PreCommitHook(_) => PRE_COMMIT,
        }
    }

    fn holds(&self, facts: &Facts) -> bool {
        match *self {
            Signal::PackageKey(key) => facts
                .package
                .as_ref()
                .is_some_and(|json| json.get(key).is_some()),
            Signal::Dependency(name) => facts.package.as_ref().is_some_and(|json| {
                ["dependencies", "devDependencies"].iter().any(|section| {
                    json.get(section)
                        .is_some_and(|deps| deps.get(name).is_some())
                })
            }),
            Signal::PyprojectTable(name) => facts.pyproject.as_ref().is_some_and(|table| {
                table
                    .get("tool")
                    .is_some_and(|tool| tool.get(name).is_some())
            }),
            Signal::PreCommitHook(id) => facts.pre_commit.as_ref().is_some_and(|text| {
                text.lines()
                    .filter_map(|line| line.trim_start_matches([' ', '-']).strip_prefix("id:"))
                    .any(|found| found.trim().trim_matches(['"', '\'']).starts_with(id))
            }),
        }
    }
}

/// One tool: its name in `tooling_ignore`, what shows it is in use in a directory, and how to
/// say what to add. Its own files come in tiers, the preferred first: the tool reads the first
/// tier that has a file in the directory and ignores the others. It is in use where one of
/// its files exists or a signal holds, and covered when any of those files, or the manifest a
/// signal is in, names the references directory.
/// `fix` takes what `Ctx` holds and gives the file to edit (it may not exist) and the lines to add.
/// `covers` says whether the text of one of those files, or of the manifest a signal is in, keeps
/// the directory out: it gets the file's name, its text and the directory.
/// Add a new tool as a row here.
struct Row {
    name: &'static str,
    files: &'static [&'static [&'static str]],
    signals: &'static [Signal],
    fix: fn(&Ctx) -> Fix,
    covers: fn(file: &str, text: &str, dir: &str) -> bool,
}

/// What `Row::fix` is given.
struct Ctx<'a> {
    /// The tool's own files that are present.
    present: &'a [&'a str],
    /// The references directory as seen from them.
    dir: &'a str,
    /// The manifests of the directory they are in.
    facts: &'a Facts,
}

/// The directory is named anywhere in the text: enough for tools with a file of their own.
fn names_dir(_file: &str, text: &str, dir: &str) -> bool {
    text.contains(dir)
}

/// Biome skips what its `includes` negate and reads `.git/info/exclude` when `vcs.enabled` and
/// `vcs.useIgnoreFile` are both on (tested: `useIgnoreFile` alone does nothing). Its configs are
/// JSONC, so a comment or a file that does not parse covers nothing.
fn biome_covers(_file: &str, text: &str, dir: &str) -> bool {
    let Some(config) = jsonc(text) else {
        return false;
    };
    let on = |key| config.pointer(&format!("/vcs/{key}")) == Some(&serde_json::Value::Bool(true));
    (on("enabled") && on("useIgnoreFile")) || names_dir_in_a_string(&config, dir)
}

/// Whether a string anywhere in `json` contains `dir`.
fn names_dir_in_a_string(json: &serde_json::Value, dir: &str) -> bool {
    match json {
        serde_json::Value::String(text) => text.contains(dir),
        serde_json::Value::Array(items) => {
            items.iter().any(|item| names_dir_in_a_string(item, dir))
        }
        serde_json::Value::Object(map) => map.values().any(|item| names_dir_in_a_string(item, dir)),
        _ => false,
    }
}

/// Black skips what `exclude`, `extend-exclude` or `force-exclude` of `[tool.black]` matches.
/// `pyproject.toml` is shared with other tools, so naming the directory elsewhere in it proves
/// nothing. Other files (a pre-commit config) are read as text.
fn black_covers(file: &str, text: &str, dir: &str) -> bool {
    if file != PYPROJECT {
        return text.contains(dir);
    }
    let Ok(table) = text.parse::<toml::Table>() else {
        return false;
    };
    let black = table.get("tool").and_then(|tool| tool.get("black"));
    ["exclude", "extend-exclude", "force-exclude"]
        .iter()
        .filter_map(|option| black?.get(option)?.as_str())
        .any(|regex| regex.contains(dir) || regex.contains(&escape_regex(dir)))
}

/// Zed hides what an entry of `file_scan_exclusions` matches. An entry may be `**/<dir>`, a path
/// from the project or an absolute one, so any entry that contains the directory counts.
fn zed_covers(_file: &str, text: &str, dir: &str) -> bool {
    jsonc(text)
        .and_then(|json| json.get("file_scan_exclusions").cloned())
        .and_then(|entries| entries.as_array().cloned())
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.as_str().is_some_and(|entry| entry.contains(dir)))
        })
}

/// `text` as JSON with comments and trailing commas, if it is.
fn jsonc(text: &str) -> Option<serde_json::Value> {
    jsonc_parser::parse_to_serde_value(text, &Default::default()).ok()
}

const TABLE: [Row; 6] = [
    Row {
        name: "prettier",
        files: &[&[
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
        ]],
        signals: &[
            Signal::PackageKey("prettier"),
            Signal::Dependency("prettier"),
            Signal::PreCommitHook("prettier"),
        ],
        fix: |Ctx { dir, .. }| (".prettierignore".into(), format!("{dir}/")),
        covers: names_dir,
    },
    Row {
        name: "eslint",
        // ESLint reads the flat config alone when there is one: it ignores `.eslintignore` and
        // the legacy `.eslintrc*` files then. With neither, the flat config is the one to create.
        files: &[
            &[
                "eslint.config.js",
                "eslint.config.mjs",
                "eslint.config.cjs",
                "eslint.config.ts",
                "eslint.config.mts",
                "eslint.config.cts",
            ],
            &[
                ".eslintrc",
                ".eslintrc.js",
                ".eslintrc.cjs",
                ".eslintrc.json",
                ".eslintrc.yaml",
                ".eslintrc.yml",
                ".eslintignore",
            ],
        ],
        signals: &[
            Signal::Dependency("eslint"),
            Signal::PreCommitHook("eslint"),
        ],
        fix: |&Ctx { present, dir, .. }| match present
            .iter()
            .find(|file| file.starts_with(".eslint"))
        {
            Some(_)
                if !present
                    .iter()
                    .any(|file| file.starts_with("eslint.config.")) =>
            {
                (".eslintignore".into(), format!("{dir}/"))
            }
            _ => {
                let flat = present
                    .iter()
                    .find(|file| file.starts_with("eslint.config."))
                    .copied()
                    .unwrap_or("eslint.config.js");
                (flat.into(), format!(r#"{{ ignores: ["{dir}/**"] }}"#))
            }
        },
        covers: names_dir,
    },
    Row {
        name: "black",
        files: &[],
        signals: &[
            Signal::PyprojectTable("black"),
            Signal::PreCommitHook("black"),
        ],
        // Black takes a regex, anchored at the project root with `^/`. The header is only for a
        // `pyproject.toml` that has no `[tool.black]` yet: a second one is invalid TOML.
        fix: |&Ctx { dir, facts, .. }| {
            let line = format!("extend-exclude = '^/{}/'", escape_regex(dir));
            let has_table = facts.pyproject.as_ref().is_some_and(|table| {
                table
                    .get("tool")
                    .is_some_and(|tool| tool.get("black").is_some())
            });
            let snippet = match has_table {
                true => line,
                false => format!(
                    "[tool.black]
{line}"
                ),
            };
            (PYPROJECT.into(), snippet)
        },
        covers: black_covers,
    },
    Row {
        name: "biome",
        files: &[&["biome.json", "biome.jsonc", ".biome.json", ".biome.jsonc"]],
        signals: &[
            Signal::Dependency("@biomejs/biome"),
            Signal::PreCommitHook("biome"),
        ],
        fix: |&Ctx { present, dir, .. }| {
            let file = present.first().copied().unwrap_or("biome.json");
            (
                file.into(),
                format!(r#""files": {{ "includes": ["**", "!!{dir}"] }}"#),
            )
        },
        covers: biome_covers,
    },
    Row {
        name: "markdownlint",
        files: &[&[
            ".markdownlint-cli2.jsonc",
            ".markdownlint-cli2.yaml",
            ".markdownlint-cli2.cjs",
            ".markdownlint-cli2.mjs",
        ]],
        signals: &[
            Signal::Dependency("markdownlint-cli2"),
            Signal::PreCommitHook("markdownlint-cli2"),
        ],
        fix: |&Ctx { present, dir, .. }| {
            let file = present
                .first()
                .copied()
                .unwrap_or(".markdownlint-cli2.jsonc");
            let snippet = match file.rsplit('.').next() {
                Some("yaml") => format!("ignores:\n  - \"{dir}/**\""),
                Some("jsonc") => format!(r#""ignores": ["{dir}/**"]"#),
                _ => format!(r#"ignores: ["{dir}/**"]"#),
            };
            (file.into(), snippet)
        },
        covers: names_dir,
    },
    Row {
        name: "zed",
        files: &[&[".zed/settings.json"]],
        signals: &[],
        fix: |&Ctx { dir, .. }| {
            (
                ".zed/settings.json".into(),
                format!(r#""file_scan_exclusions": ["**/{dir}", "..."]"#),
            )
        },
        covers: zed_covers,
    },
];

/// Names earlier versions reported. `tooling_ignore` still accepts them and they do nothing: the
/// tools skip the references directory by themselves (spec 7.6).
const RETIRED: [&str; 3] = ["oxlint", "tsc", "stylelint"];

/// The names `tooling_ignore` accepts.
pub fn names() -> Vec<&'static str> {
    TABLE.iter().map(|row| row.name).chain(RETIRED).collect()
}

/// Where a tool is in use: how far up from the project directory, and what shows it.
struct Found<'a> {
    up: usize,
    dir: &'a Path,
    /// The tool's own files that are there.
    present: Vec<&'static str>,
    /// The manifests its signals are in.
    manifests: Vec<&'static str>,
}

impl Row {
    /// The nearest of `dirs` (with its facts) in which the tool is in use.
    fn find<'a>(&self, dirs: &'a [(PathBuf, Facts)]) -> Option<Found<'a>> {
        dirs.iter().enumerate().find_map(|(up, (dir, facts))| {
            let present = self.files.iter().find_map(|tier| {
                let present: Vec<&str> = tier
                    .iter()
                    .copied()
                    .filter(|file| dir.join(file).is_file())
                    .collect();
                (!present.is_empty()).then_some(present)
            });
            let mut manifests: Vec<&str> = Vec::new();
            for signal in self.signals.iter().filter(|signal| signal.holds(facts)) {
                if !manifests.contains(&signal.file()) {
                    manifests.push(signal.file());
                }
            }
            (present.is_some() || !manifests.is_empty()).then(|| Found {
                up,
                dir: dir.as_path(),
                present: present.unwrap_or_default(),
                manifests,
            })
        })
    }
}

/// The tools in use for the project at `project_dir` whose files do not name `references_dir`,
/// except `ignored`. Each tool is read in the nearest directory that shows it in use, from
/// `project_dir` up to the top of its git worktree (`candidate_dirs`).
pub fn gaps(project_dir: &Path, references_dir: &str, ignored: &[&str]) -> Vec<Gap> {
    let dirs: Vec<(PathBuf, Facts)> = candidate_dirs(project_dir)
        .into_iter()
        .map(|dir| {
            let facts = Facts::read(&dir);
            (dir, facts)
        })
        .collect();
    let mut found = Vec::new();
    for row in TABLE.iter().filter(|row| !ignored.contains(&row.name)) {
        let Some(Found {
            up,
            dir,
            present,
            manifests,
        }) = row.find(&dirs)
        else {
            continue;
        };
        let facts = &dirs[up].1;
        let (file, snippet) = (row.fix)(&Ctx {
            present: &present,
            dir: &seen_from(dir, project_dir, references_dir),
            facts,
        });
        // The file to edit may be beside the ones that showed the tool in use.
        let covered = present
            .iter()
            .chain(&manifests)
            .copied()
            .chain([file.as_str()])
            .any(|file| {
                std::fs::read_to_string(dir.join(file))
                    .is_ok_and(|text| (row.covers)(file, &text, references_dir))
            });
        if !covered {
            found.push(Gap {
                tool: row.name,
                create: !dir.join(&file).is_file(),
                file: format!("{}{file}", "../".repeat(up)),
                snippet,
            });
        }
    }
    found
}

/// `text` as a regex that matches it literally.
fn escape_regex(text: &str) -> String {
    text.chars()
        .flat_map(|c| match c {
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' => {
                vec!['\\', c]
            }
            _ => vec![c],
        })
        .collect()
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
