//! `refs add` (`add::run`), then its report.

use std::path::{Path, PathBuf};

use super::{load, with_source};
use crate::add::Asking;
use crate::config::Config;
use crate::diagnostic::{ProjectError, SourceError};
use crate::edit::AddRepo;
use crate::project;
use crate::source::Source;
use crate::sync::SyncFlags;
use crate::ui::{Console, fail, failed, print_report};

pub(crate) fn run<'a>(
    start: &Path,
    make_source: impl FnOnce(&Path, &Config) -> Result<Box<dyn Source + 'a>, SourceError>,
    repo: &AddRepo,
    no_sync: bool,
    flags: &SyncFlags,
    mut asking: Asking,
    console: &mut Console,
) -> u8 {
    let (root, config) = match load_or_create(start, console) {
        Ok(project) => project,
        Err(code) => return code,
    };
    let mut add = |source: Option<&dyn Source>, console: &mut Console| match crate::add::run(
        source,
        &root,
        &config,
        repo,
        flags,
        &mut asking,
        console,
    ) {
        Ok(report) => print_report(&report, false, console),
        Err(failure) => failed(failure, console),
    };
    if no_sync {
        add(None, console)
    } else {
        with_source(make_source, &root, &config, console, |source, console| {
            add(Some(source), console)
        })
    }
}

/// `load`, except that with no `refs.toml` anywhere up the project is the place `refs init`
/// would set one up and an empty config: the edit creates the file (ADR 0005).
fn load_or_create(start: &Path, console: &mut Console) -> Result<(PathBuf, Config), u8> {
    let no_config = matches!(
        project::find_root(start),
        Err(ProjectError::NoConfig { .. })
    );
    if !no_config {
        return load(start, console);
    }
    let root = project::init_root(start, false).map_err(|e| fail(e, console))?;
    Ok((root, Config::default()))
}
