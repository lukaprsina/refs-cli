//! `refs list`, and with `--status` the config, the Lock and the Checkout state of each active
//! repo.

use std::path::Path;

use super::{load, with_source};
use crate::config::Config;
use crate::diagnostic::SourceError;
use crate::source::Source;
use crate::ui::{Console, fail_all};

pub(crate) fn run<'a>(
    start: &Path,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    status: bool,
    color: bool,
    console: &mut Console,
) -> u8 {
    let (root, config) = match load(start, console) {
        Ok(project) => project,
        Err(code) => return code,
    };
    if status {
        return with_source(make_source, &root, &config, console, |source, console| {
            match crate::status::build(source, &root, &config) {
                Ok(status) => {
                    let table = crate::list::list_status(&config, |laid| status.row(laid), color);
                    console.data(&table);
                    0
                }
                Err(reports) => fail_all(reports, console),
            }
        });
    }
    console.data(&crate::list::list(&config, color));
    0
}
