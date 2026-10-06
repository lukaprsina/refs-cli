//! Whole-file writes that a concurrent reader never sees half done.

use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::time::Duration;

use tempfile::{Builder, NamedTempFile};

/// How often, and how far apart, a rename refused for access is tried again (Windows only).
const RENAME_RETRIES: u32 = 100;
const RENAME_BACKOFF: Duration = Duration::from_millis(10);

/// Write `contents` to a uniquely named temp file beside `path`, then rename it into place;
/// the temp file shares the directory so the rename never crosses a file system, and is
/// removed if anything fails. The existing file's permissions carry over; a new file gets
/// what a plain create would.
pub fn write(path: &Path, contents: &str) -> io::Result<()> {
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut tmp = builder().tempfile_in(dir)?;
    tmp.write_all(contents.as_bytes())?;
    if let Ok(meta) = std::fs::metadata(path) {
        std::fs::set_permissions(tmp.path(), meta.permissions())?;
    }
    persist(tmp, path)
}

/// The temp file would be private (0600) by default; the kernel applies the umask to this.
#[cfg(unix)]
fn builder() -> Builder<'static, 'static> {
    use std::os::unix::fs::PermissionsExt;
    let mut builder = Builder::new();
    builder.permissions(std::fs::Permissions::from_mode(0o666));
    builder
}

#[cfg(not(unix))]
fn builder() -> Builder<'static, 'static> {
    Builder::new()
}

/// Rename `tmp` over `path`. Windows refuses (access denied) while another process is
/// replacing or reading the same file, which passes in moments, so it is tried again.
fn persist(mut tmp: NamedTempFile, path: &Path) -> io::Result<()> {
    let mut tries = 0;
    loop {
        match tmp.persist(path) {
            Ok(_) => return Ok(()),
            Err(e)
                if cfg!(windows)
                    && e.error.kind() == ErrorKind::PermissionDenied
                    && tries < RENAME_RETRIES =>
            {
                tries += 1;
                tmp = e.file;
                std::thread::sleep(RENAME_BACKOFF);
            }
            Err(e) => return Err(e.error),
        }
    }
}
