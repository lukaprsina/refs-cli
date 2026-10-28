//! The JavaScript tools: Prettier, ESLint, Biome and markdownlint-cli2.

use super::Finding;
use super::site::{PACKAGE_JSON, PRE_COMMIT, Site, mentions};

const PRETTIER: &[&str] = &[
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
];

pub(super) fn prettier(site: &Site) -> Finding {
    let signals = [
        (
            PACKAGE_JSON,
            site.package_key("prettier") || site.dependency("prettier"),
        ),
        (PRE_COMMIT, site.hook("prettier")),
    ];
    let Some(mut files) = site.in_use(&site.present(PRETTIER), &signals) else {
        return Finding::Absent;
    };
    files.push(".prettierignore");
    match site.any_names_dir(&files) {
        true => Finding::Covered,
        false => Finding::gap(".prettierignore", format!("{}/", site.seen)),
    }
}

/// ESLint reads the flat config alone when there is one: it ignores `.eslintignore` and the
/// legacy `.eslintrc*` files then. With neither, the flat config is the one to create.
const ESLINT_FLAT: &[&str] = &[
    "eslint.config.js",
    "eslint.config.mjs",
    "eslint.config.cjs",
    "eslint.config.ts",
    "eslint.config.mts",
    "eslint.config.cts",
];
const ESLINT_LEGACY: &[&str] = &[
    ".eslintrc",
    ".eslintrc.js",
    ".eslintrc.cjs",
    ".eslintrc.json",
    ".eslintrc.yaml",
    ".eslintrc.yml",
    ".eslintignore",
];

pub(super) fn eslint(site: &Site) -> Finding {
    let flat = site.present(ESLINT_FLAT);
    let legacy = site.present(ESLINT_LEGACY);
    let own = if flat.is_empty() { &legacy } else { &flat };
    let signals = [
        (PACKAGE_JSON, site.dependency("eslint")),
        (PRE_COMMIT, site.hook("eslint")),
    ];
    let Some(mut files) = site.in_use(own, &signals) else {
        return Finding::Absent;
    };
    let (file, snippet) = match (flat.first(), legacy.is_empty()) {
        (None, false) => (".eslintignore", format!("{}/", site.seen)),
        (flat, _) => (
            flat.copied().unwrap_or("eslint.config.js"),
            format!(r#"{{ ignores: ["{}/**"] }}"#, site.seen),
        ),
    };
    files.push(file);
    match site.any_names_dir(&files) {
        true => Finding::Covered,
        false => Finding::gap(file, snippet),
    }
}

const BIOME: &[&str] = &["biome.json", "biome.jsonc", ".biome.json", ".biome.jsonc"];

/// Biome skips what its `includes` negate and reads `.git/info/exclude` when `vcs.enabled` and
/// `vcs.useIgnoreFile` are both on (tested: `useIgnoreFile` alone does nothing). Its configs are
/// JSONC, so a comment or a file that does not parse covers nothing.
pub(super) fn biome(site: &Site) -> Finding {
    let own = site.present(BIOME);
    let signals = [
        (PACKAGE_JSON, site.dependency("@biomejs/biome")),
        (PRE_COMMIT, site.hook("biome")),
    ];
    if site.in_use(&own, &signals).is_none() {
        return Finding::Absent;
    }
    let file = own.first().copied().unwrap_or("biome.json");
    let covers = |config: serde_json::Value| {
        let on =
            |key| config.pointer(&format!("/vcs/{key}")) == Some(&serde_json::Value::Bool(true));
        (on("enabled") && on("useIgnoreFile")) || mentions(&config, &site.name)
    };
    // Only the config says what Biome skips: not `package.json` or the hook, whatever shows
    // Biome is in use.
    match own.iter().any(|file| site.jsonc(file).is_some_and(covers)) {
        true => Finding::Covered,
        false => Finding::gap(
            file,
            format!(r#""files": {{ "includes": ["**", "!!{}"] }}"#, site.seen),
        ),
    }
}

const MARKDOWNLINT: &[&str] = &[
    ".markdownlint-cli2.jsonc",
    ".markdownlint-cli2.yaml",
    ".markdownlint-cli2.cjs",
    ".markdownlint-cli2.mjs",
];

pub(super) fn markdownlint(site: &Site) -> Finding {
    let own = site.present(MARKDOWNLINT);
    let signals = [
        (PACKAGE_JSON, site.dependency("markdownlint-cli2")),
        (PRE_COMMIT, site.hook("markdownlint-cli2")),
    ];
    let Some(mut files) = site.in_use(&own, &signals) else {
        return Finding::Absent;
    };
    let file = own.first().copied().unwrap_or(".markdownlint-cli2.jsonc");
    let dir = &site.seen;
    let snippet = match file.rsplit('.').next() {
        Some("yaml") => format!("ignores:\n  - \"{dir}/**\""),
        Some("jsonc") => format!(r#""ignores": ["{dir}/**"]"#),
        _ => format!(r#"ignores: ["{dir}/**"]"#),
    };
    files.push(file);
    match site.any_names_dir(&files) {
        true => Finding::Covered,
        false => Finding::gap(file, snippet),
    }
}
