//! Whole-file writes that a concurrent reader never sees half done.

use std::io;
use std::path::Path;

/// Write `contents` to a temp file beside `path`, then rename it into place. The existing
/// file's permissions carry over. The temp file is removed if anything fails.
pub fn write(path: &Path, contents: &str) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = Path::new(&tmp);
    let result = std::fs::write(tmp, contents)
        .and_then(|()| match std::fs::metadata(path) {
            Ok(meta) => std::fs::set_permissions(tmp, meta.permissions()),
            Err(_) => Ok(()),
        })
        .and_then(|()| std::fs::rename(tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
