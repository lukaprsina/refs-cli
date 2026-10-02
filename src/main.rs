use std::process::ExitCode;

use refs_cli::config::RepoRef;
use refs_cli::diagnostic::SourceError;
use refs_cli::source::{MaterialiseOpts, Observed, Pin, Source};

/// Stands in until `GitSource` lands (milestone 1: #2, #7, #8): every operation fails.
struct NoGit;

impl NoGit {
    fn unavailable<T>() -> Result<T, SourceError> {
        Err(SourceError::Failed {
            message: "git support is not implemented yet".into(),
        })
    }
}

impl Source for NoGit {
    fn resolve(&self, _: RepoRef) -> Result<Pin, SourceError> {
        Self::unavailable()
    }
    fn verify(&self, _: RepoRef, _: &Pin) -> Result<(), SourceError> {
        Self::unavailable()
    }
    fn materialise(&self, _: RepoRef, _: &Pin, _: MaterialiseOpts) -> Result<(), SourceError> {
        Self::unavailable()
    }
    fn remove(&self, _: &str) -> Result<(), SourceError> {
        Self::unavailable()
    }
    fn inspect(&self, _: &str) -> Result<Observed, SourceError> {
        Self::unavailable()
    }
    fn list(&self) -> Result<Vec<String>, SourceError> {
        Self::unavailable()
    }
}

fn main() -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("cannot read the current directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    ExitCode::from(refs_cli::cli::run(std::env::args_os(), &cwd, &NoGit))
}
