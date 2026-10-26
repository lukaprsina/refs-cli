//! `refs remove`, `refs disable` and `refs enable`: an edit of `refs.toml`, then a sync unless
//! `--no-sync`.

use std::path::Path;

use super::{load, with_source};
use crate::config::Config;
use crate::diagnostic::SourceError;
use crate::edit::Edit;
use crate::source::Source;
use crate::sync::{self, SyncFlags};
use crate::ui::{Console, print_report};

pub(crate) fn run<'a>(
    start: &Path,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    edit: &Edit,
    no_sync: bool,
    flags: &SyncFlags,
    console: &mut Console,
) -> u8 {
    let project = match load(start, console) {
        Ok(project) => project,
        Err(code) => return code,
    };
    if no_sync {
        return print_report(&sync::edit_only(&project.root, edit), false, console);
    }
    with_source(make_source, &project, console, |source, console| {
        let report = sync::edit(source, &project.root, edit, flags);
        print_report(&report, false, console)
    })
}
