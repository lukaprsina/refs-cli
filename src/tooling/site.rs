//! One candidate directory as the tools see it: its manifests read once, its files on demand.

use std::path::{Path, PathBuf};

pub(super) const PACKAGE_JSON: &str = "package.json";
pub(super) const PYPROJECT: &str = "pyproject.toml";
pub(super) const PRE_COMMIT: &str = ".pre-commit-config.yaml";

/// A directory a tool may be configured in. Nothing is read from lockfiles (transitive
/// dependencies) or from the Python manifests but the `[tool.*]` tables. A file that cannot be
/// read or parsed says nothing.
pub(super) struct Site {
    pub(super) dir: PathBuf,
    /// The references directory by its own name, as the tools' files spell it.
    pub(super) name: String,
    /// The references directory as a `/`-separated path from `dir`.
    pub(super) seen: String,
    package: Option<serde_json::Value>,
    pyproject: Option<toml::Table>,
    pre_commit: Option<String>,
}

impl Site {
    /// `dir`, an ancestor of `project_dir` (or the same), where `name` is the references directory
    /// of the project.
    pub(super) fn read(dir: PathBuf, project_dir: &Path, name: &str) -> Site {
        let text = |file: &str| std::fs::read_to_string(dir.join(file)).ok();
        let below = project_dir.strip_prefix(&dir).unwrap_or(Path::new(""));
        let mut parts: Vec<String> = below
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        parts.push(name.to_owned());
        Site {
            package: text(PACKAGE_JSON).and_then(|text| serde_json::from_str(&text).ok()),
            pyproject: text(PYPROJECT).and_then(|text| text.parse().ok()),
            pre_commit: text(PRE_COMMIT),
            seen: parts.join("/"),
            name: name.to_owned(),
            dir,
        }
    }

    /// The files of one tier of a tool's own files that are here, in the tier's order. The tool
    /// reads the first tier that has a file and ignores the others.
    pub(super) fn present(&self, tier: &[&'static str]) -> Vec<&'static str> {
        tier.iter()
            .copied()
            .filter(|file| self.dir.join(file).is_file())
            .collect()
    }

    /// The text of `file`.
    pub(super) fn text(&self, file: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.join(file)).ok()
    }

    /// `file` as JSON with comments and trailing commas, if it is.
    pub(super) fn jsonc(&self, file: &str) -> Option<serde_json::Value> {
        jsonc_parser::parse_to_serde_value(&self.text(file)?, &Default::default()).ok()
    }

    /// Whether the text of any of `files` names the references directory.
    pub(super) fn any_names_dir(&self, files: &[&str]) -> bool {
        files.iter().any(|file| {
            self.text(file)
                .is_some_and(|text| text.contains(&self.name))
        })
    }

    /// Whether a line of the ignore file `file` is the references directory: `.references`,
    /// `/.references/`, or the path to it from here. A comment, a negation or a longer path is not.
    /// A line with no leading `/` also covers by the bare name, which matches at any depth; one
    /// with it is anchored here.
    pub(super) fn lists_dir(&self, file: &str) -> bool {
        let Some(text) = self.text(file) else {
            return false;
        };
        text.lines().any(|line| {
            let line = line.trim().trim_start_matches('\u{feff}');
            let path = line.strip_prefix('/').unwrap_or(line).trim_end_matches('/');
            path == self.seen || (path == self.name && !line.starts_with('/'))
        })
    }

    /// A top-level key of `package.json`.
    pub(super) fn package_key(&self, key: &str) -> bool {
        self.package
            .as_ref()
            .is_some_and(|json| json.get(key).is_some())
    }

    /// A direct entry in `dependencies` or `devDependencies` of `package.json`.
    pub(super) fn dependency(&self, name: &str) -> bool {
        self.package.as_ref().is_some_and(|json| {
            ["dependencies", "devDependencies"].iter().any(|section| {
                json.get(section)
                    .is_some_and(|deps| deps.get(name).is_some())
            })
        })
    }

    /// The table `[tool.<name>]` of `pyproject.toml`.
    pub(super) fn pyproject_table(&self, name: &str) -> Option<&toml::Value> {
        self.pyproject.as_ref()?.get("tool")?.get(name)
    }

    /// A hook whose id starts with `id`, in `.pre-commit-config.yaml`.
    pub(super) fn hook(&self, id: &str) -> bool {
        self.pre_commit.as_ref().is_some_and(|text| {
            text.lines()
                .filter_map(|line| line.trim_start_matches([' ', '-']).strip_prefix("id:"))
                .any(|found| found.trim().trim_matches(['"', '\'']).starts_with(id))
        })
    }

    /// The files that show a tool is in use here, or `None` if nothing does: its own `present`
    /// files and the manifests whose signal holds. Besides its own files, a tool is shown by a
    /// key or entry in a manifest, or a hook in the pre-commit config.
    pub(super) fn in_use(
        &self,
        present: &[&'static str],
        signals: &[(&'static str, bool)],
    ) -> Option<Vec<&'static str>> {
        let mut files = present.to_vec();
        for &(file, holds) in signals {
            if holds && !files.contains(&file) {
                files.push(file);
            }
        }
        (!files.is_empty()).then_some(files)
    }
}

/// Whether a string anywhere in `json` satisfies `test`.
pub(super) fn any_string(json: &serde_json::Value, test: &dyn Fn(&str) -> bool) -> bool {
    match json {
        serde_json::Value::String(text) => test(text),
        serde_json::Value::Array(items) => items.iter().any(|item| any_string(item, test)),
        serde_json::Value::Object(map) => map.values().any(|item| any_string(item, test)),
        _ => false,
    }
}
