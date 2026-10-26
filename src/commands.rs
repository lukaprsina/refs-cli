//! One driver per command, in the order of the clap tree: load the project, build the `Source`
//! when the command syncs, run, print the report and give the exit code. What a command decides is
//! in `sync`, `plan`, `add` and `update`; `cli` parses the arguments into plain values and calls
//! one driver. `remove`, `enable` and `disable` share `edit`.

pub(crate) mod add;
pub(crate) mod edit;
pub(crate) mod init;
pub(crate) mod list;
pub(crate) mod self_update;
pub(crate) mod sync;
pub(crate) mod update;

use std::path::Path;

use crate::config::Config;
use crate::diagnostic::SourceError;
use crate::project::Project;
use crate::source::Source;
use crate::ui::{Console, fail, fail_all};

/// The project at or above `start`; on failure the problems are printed and the exit code
/// returned.
fn load(start: &Path, console: &mut Console) -> Result<Project, u8> {
    crate::project::load(start).map_err(|reports| fail_all(reports, console))
}

/// Build the `Source` and run `f` with it. Only the commands that sync ask for one, so the
/// others do not need a cache directory.
fn with_source<'a>(
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    project: &Project,
    console: &mut Console,
    f: impl FnOnce(&dyn Source, &mut Console) -> u8,
) -> u8 {
    match make_source(&project.root, &project.config) {
        Ok(source) => f(&*source, console),
        Err(e) => fail(e, console),
    }
}
