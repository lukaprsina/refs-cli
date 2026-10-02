//! Marker parsing and splicing for Agent files. Pure functions over text.

use std::path::Path;

use crate::diagnostic::BlockError;

const BEGIN: &str = "<!-- BEGIN:refs -->";
const END: &str = "<!-- END:refs -->";

/// Put `block` (markers included, `\n` line ends) into `text`: replace the existing
/// marked region, or append after a blank line when there is none. Everything outside
/// the markers is kept byte for byte. The block takes the line ending of the region it
/// replaces (of the whole file when appending), so splicing what is already there
/// changes nothing even in a file with mixed endings.
pub fn splice(text: &str, block: &str) -> Result<String, BlockError> {
    let region = find_region(text)?;
    let sample = region.as_ref().map_or(text, |r| &text[r.clone()]);
    let eol = if sample.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let block = block.replace("\r\n", "\n").replace('\n', eol);
    match region {
        Some(region) => Ok(format!(
            "{}{block}{}",
            &text[..region.start],
            &text[region.end..]
        )),
        None if text.is_empty() => Ok(format!("{block}{eol}")),
        None if text.ends_with('\n') => Ok(format!("{text}{eol}{block}{eol}")),
        None => Ok(format!("{text}{eol}{eol}{block}{eol}")),
    }
}

/// Byte range from the start of the BEGIN marker to the end of the END marker.
fn find_region(text: &str) -> Result<Option<std::ops::Range<usize>>, BlockError> {
    // A marker is a whole line, so prose that quotes one is left alone.
    let mut markers = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset + (line.len() - line.trim_start().len());
        let marker = line.trim();
        if marker == BEGIN || marker == END {
            markers.push((marker == BEGIN, start..start + marker.len()));
        }
        offset += line.len();
    }
    match markers.as_slice() {
        [] => Ok(None),
        [(true, begin), (false, end)] => Ok(Some(begin.start..end.end)),
        [_] => Err(BlockError::Unbalanced),
        [(false, _), ..] => Err(BlockError::Reversed),
        [(true, _), (true, _), ..] => Err(BlockError::Nested),
        _ => Err(BlockError::Duplicated),
    }
}

/// Write via a temp file in the same directory, then rename, so a reader never sees a
/// partial file. A symlinked Agent file (`CLAUDE.md` pointing at `AGENTS.md`) is written
/// through, so the link survives.
pub fn write(path: &Path, text: &str) -> Result<(), BlockError> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    crate::atomic::write(&target, text).map_err(|source| BlockError::Write {
        path: path.display().to_string(),
        source,
    })
}
