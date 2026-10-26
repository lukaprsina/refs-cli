//! `refs update`, then its report and the count of the Repos it skipped.

use std::path::Path;

use super::{load, with_source};
use crate::config::Config;
use crate::diagnostic::SourceError;
use crate::source::Source;
use crate::ui::{Console, Prompter, failed, print_report, skipped};
use crate::update;

pub(crate) fn run<'a>(
    start: &Path,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    request: &update::Request,
    prompter: Option<&mut dyn Prompter>,
    console: &mut Console,
) -> u8 {
    let project = match load(start, console) {
        Ok(project) => project,
        Err(code) => return code,
    };
    with_source(
        make_source,
        &project,
        console,
        |source, console| match update::run(
            source,
            &project.root,
            &project.config,
            request,
            prompter,
            console,
        ) {
            Ok(done) => {
                console.data(&done.listing);
                let code = done
                    .report
                    .map_or(0, |report| print_report(&report, false, console));
                skipped(done.skipped, code, console)
            }
            Err(failure) => failed(failure, console),
        },
    )
}
