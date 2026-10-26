//! `refs sync`.

use std::path::Path;

use super::{load, with_source};
use crate::config::Config;
use crate::diagnostic::SourceError;
use crate::source::Source;
use crate::sync::{self, SyncFlags};
use crate::ui::{Console, print_report};

pub(crate) fn run<'a>(
    start: &Path,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    flags: &SyncFlags,
    console: &mut Console,
) -> u8 {
    let (root, config) = match load(start, console) {
        Ok(project) => project,
        Err(code) => return code,
    };
    with_source(make_source, &root, &config, console, |source, console| {
        let report = sync::sync(source, &root, &config, flags);
        print_report(&report, flags.check, console)
    })
}
