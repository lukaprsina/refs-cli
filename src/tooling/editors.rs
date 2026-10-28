//! The editors: Zed.

use super::Finding;
use super::site::{Site, mentions};

const SETTINGS: &str = ".zed/settings.json";

/// Zed hides what an entry of `file_scan_exclusions` matches. An entry may be `**/<dir>`, a path
/// from the project or an absolute one, so any entry that contains the directory counts.
pub(super) fn zed(site: &Site) -> Finding {
    if site.present(&[SETTINGS]).is_empty() {
        return Finding::Absent;
    }
    let covered = site
        .jsonc(SETTINGS)
        .and_then(|json| json.get("file_scan_exclusions").cloned())
        .is_some_and(|entries| entries.is_array() && mentions(&entries, &site.name));
    match covered {
        true => Finding::Covered,
        false => Finding::gap(
            SETTINGS,
            format!(r#""file_scan_exclusions": ["**/{}", "..."]"#, site.seen),
        ),
    }
}
