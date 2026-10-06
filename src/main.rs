use std::process::ExitCode;

use refs_cli::source::git::GitSource;

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
            let checkouts = root.join(config.settings.references_dir());
            Ok(Box::new(GitSource::from_env(checkouts)?))
        },
    ))
}
