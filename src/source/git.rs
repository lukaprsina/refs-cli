//! `Source` backed by the system `git` binary. Every git command goes through `run`, which
//! is where the safety rules of spec §7.7 and the version floor of §7.5 are enforced, so no
//! call site can skip them.

pub mod remote;

use std::process::Command;
use std::sync::OnceLock;

use crate::config::{RepoRef, is_full_sha};
use crate::diagnostic::SourceError;
use crate::source::{MaterialiseOpts, Observed, Pin, Source};

/// Transports git may use; `ext::` and the like are refused (spec §7.7).
const ALLOWED_PROTOCOLS: &str = "file:https:ssh:git";

#[derive(Default)]
pub struct GitSource {
    /// The outcome of the one `git --version` check, made before the first command.
    version: OnceLock<Result<(), SourceError>>,
}

impl GitSource {
    pub fn new() -> GitSource {
        GitSource::default()
    }

    /// Run `git <args>` and return its stdout. `args` must already have `--` before any
    /// user-supplied value.
    fn git(&self, args: &[&str]) -> Result<String, SourceError> {
        self.version
            .get_or_init(|| remote::check_version(&run(&["--version"])?))
            .clone()?;
        run(args)
    }
}

fn run(args: &[&str]) -> Result<String, SourceError> {
    let failed = |message: String| SourceError::Failed { message };
    let output = Command::new("git")
        .args(args)
        .env("GIT_ALLOW_PROTOCOL", ALLOWED_PROTOCOLS)
        .output()
        .map_err(|e| failed(format!("could not run git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let command = args
            .iter()
            .take_while(|a| **a != "--")
            .copied()
            .collect::<Vec<_>>();
        return Err(failed(format!(
            "git {} failed: {}",
            command.join(" "),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

impl Source for GitSource {
    fn resolve(&self, repo: RepoRef) -> Result<Pin, SourceError> {
        let url = repo.repo.url.as_ref().as_str();
        let git_ref = repo.repo.effective_ref();
        remote::check_input(url, git_ref)?;
        if is_full_sha(git_ref) {
            return Ok(Pin::git(url, git_ref, git_ref, None));
        }
        if git_ref == "HEAD" {
            let out = self.git(&["ls-remote", "--symref", "--", url, "HEAD"])?;
            let (sha, branch) = remote::select_head(url, &out)?;
            return Ok(Pin::git(url, git_ref, &sha, branch.as_deref()));
        }
        let patterns = [
            format!("refs/heads/{git_ref}"),
            format!("refs/tags/{git_ref}"),
            format!("refs/tags/{git_ref}^{{}}"),
        ];
        let mut args = vec!["ls-remote", "--", url];
        args.extend(patterns.iter().map(String::as_str));
        let out = self.git(&args)?;
        let sha = remote::select_ref(url, git_ref, &out)?;
        Ok(Pin::git(url, git_ref, &sha, None))
    }

    fn verify(&self, _: RepoRef, _: &Pin) -> Result<(), SourceError> {
        not_implemented()
    }

    fn materialise(&self, _: RepoRef, _: &Pin, _: MaterialiseOpts) -> Result<(), SourceError> {
        not_implemented()
    }

    fn remove(&self, _: &str) -> Result<(), SourceError> {
        not_implemented()
    }

    fn inspect(&self, _: &str) -> Result<Observed, SourceError> {
        not_implemented()
    }

    fn list(&self) -> Result<Vec<String>, SourceError> {
        not_implemented()
    }
}

/// The cache and checkouts land in #7 and #8.
fn not_implemented<T>() -> Result<T, SourceError> {
    Err(SourceError::Failed {
        message: "git support is not implemented yet".into(),
    })
}
