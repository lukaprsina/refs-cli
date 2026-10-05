//! One place that builds and runs a `git` process, so the safety rules of spec §7.7 (the
//! protocol allowlist) hold for every call and none can skip them.

use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use super::remote::check_version;
use crate::diagnostic::SourceError;

/// Transports git may use; `ext::` and the like are refused (spec §7.7).
const ALLOWED_PROTOCOLS: &str = "file:https:ssh:git";

/// The outcome of the one `git --version` check, made before the first command (spec §7.5).
static VERSION: OnceLock<Result<(), SourceError>> = OnceLock::new();

/// A git command that failed to run or exited non-zero. `stderr` is kept so the caller can
/// recognise the few failures the spec treats specially.
#[derive(Debug)]
pub struct Failure {
    pub error: SourceError,
    pub stderr: String,
}

impl From<Failure> for SourceError {
    fn from(failure: Failure) -> SourceError {
        failure.error
    }
}

pub struct Cmd {
    command: Command,
    /// The arguments before any `--`, for the error message: what follows is user input.
    shown: Vec<String>,
    stdin: Option<String>,
}

/// Variables that point git at a particular repository, index or object store.
const INHERITED_REPOSITORY_VARS: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
];

impl Cmd {
    pub fn new() -> Cmd {
        let mut command = Command::new("git");
        // Windows git refuses paths past MAX_PATH without this; deep Checkouts reach it.
        command.args(["-c", "core.longpaths=true"]);
        // A caller's repository (set by git itself inside hooks) must not redirect ours.
        for var in INHERITED_REPOSITORY_VARS {
            command.env_remove(var);
        }
        command
            .env("GIT_ALLOW_PROTOCOL", ALLOWED_PROTOCOLS)
            .env("GIT_TERMINAL_PROMPT", "0");
        Cmd {
            command,
            shown: Vec::new(),
            stdin: None,
        }
    }

    /// Run in `dir`, as `git -C <dir>` does.
    pub fn dir(mut self, dir: &Path) -> Cmd {
        self.command.current_dir(dir);
        self
    }

    /// Act on the bare repository at `git_dir`, given explicitly so that a user's
    /// `safe.bareRepository=explicit` does not stop it being found.
    pub fn git_dir(mut self, git_dir: &Path) -> Cmd {
        self.command.arg("--git-dir").arg(git_dir);
        self
    }

    /// Never fetch a missing object from the promisor remote: it is an error instead.
    pub fn no_lazy_fetch(mut self) -> Cmd {
        self.command.env("GIT_NO_LAZY_FETCH", "1");
        self
    }

    /// Find the repository from the directory alone, not from `GIT_DIR` and the like that a
    /// git hook or a wrapper left in the environment.
    pub fn own_repository(mut self) -> Cmd {
        for var in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
        ] {
            self.command.env_remove(var);
        }
        self
    }

    pub fn stdin(mut self, input: String) -> Cmd {
        self.stdin = Some(input);
        self
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Cmd {
        let arg = arg.as_ref();
        if !self.shown.iter().any(|a| a == "--") {
            self.shown.push(arg.to_string_lossy().into_owned());
        }
        self.command.arg(arg);
        self
    }

    pub fn args<I>(self, args: I) -> Cmd
    where
        I: IntoIterator,
        I::Item: AsRef<OsStr>,
    {
        args.into_iter().fold(self, Cmd::arg)
    }

    /// Run it and return its stdout.
    pub fn run(mut self) -> Result<String, Failure> {
        let failed = |message: String, stderr: String| Failure {
            error: SourceError::Failed { message },
            stderr,
        };
        let command = self.shown.join(" ");
        if command != "--version" {
            let version = VERSION.get_or_init(|| {
                let out = Cmd::new()
                    .arg("--version")
                    .run()
                    .map_err(SourceError::from)?;
                check_version(&out)
            });
            version.clone().map_err(|error| Failure {
                error,
                stderr: String::new(),
            })?;
        }
        self.command
            .stdin(if self.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = self
            .command
            .spawn()
            .map_err(|e| failed(format!("could not run git: {e}"), String::new()))?;
        if let Some(input) = &self.stdin {
            let mut stdin = child.stdin.take().expect("stdin was piped");
            // A git that exits early closes the pipe; its exit status says why.
            let _ = stdin.write_all(input.as_bytes());
        }
        let output = child
            .wait_with_output()
            .map_err(|e| failed(format!("could not run git: {e}"), String::new()))?;
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if !output.status.success() {
            return Err(failed(
                format!("git {command} failed: {}", stderr.trim()),
                stderr,
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
