use std::path::PathBuf;
use std::process::ExitCode;

use refs_cli::diagnostic::SourceError;
use refs_cli::source::git::GitSource;
use refs_cli::source::git::cache::cache_root;

fn main() -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("cannot read the current directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    ExitCode::from(refs_cli::cli::run(
        std::env::args_os(),
        &cwd,
        |root, config| {
            let local_app_data = if cfg!(windows) {
                std::env::var("LOCALAPPDATA").ok()
            } else {
                None
            };
            let cache = cache_root(
                std::env::var("XDG_CACHE_HOME").ok().as_deref(),
                local_app_data.as_deref(),
                std::env::var("HOME").ok().as_deref(),
            )
            .ok_or_else(|| SourceError::Failed {
                message: "cannot find the cache directory: set XDG_CACHE_HOME (or LOCALAPPDATA \
                          on Windows, or HOME)"
                    .into(),
            })?;
            let checkouts: PathBuf = root.join(config.settings.references_dir());
            Ok(Box::new(GitSource::new(cache, checkouts)))
        },
    ))
}
