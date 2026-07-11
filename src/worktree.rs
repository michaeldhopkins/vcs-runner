//! Working-tree file access.
//!
//! The working copy lives on disk, so its content is read directly here rather
//! than through jj or git. A direct disk read is the working-copy-agnostic way
//! to get the "working" (current) side of a diff: it never snapshots the jj
//! working copy, so a long-lived reader — a TUI refreshing on every save —
//! doesn't churn the operation log or race the user's foreground jj commands.
//! Pairs with [`crate::run_jj_utf8_ignore_wc`], which covers the committed/repo
//! side without snapshotting; between them a consumer can show a live diff
//! (committed base vs on-disk working copy) while writing nothing.
//!
//! `rel_path` is always relative to `repo_path`. An absent file yields
//! `Ok(None)` (it reads as deleted), so callers don't special-case existence.

use std::path::Path;

/// Read a working-tree file's current on-disk content as text (lossy UTF-8).
/// `Ok(None)` when the file is absent (e.g. deleted); `Err` only on a real I/O
/// error.
pub fn read_working_file(repo_path: &Path, rel_path: &str) -> std::io::Result<Option<String>> {
    match std::fs::read(repo_path.join(rel_path)) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Bytes variant of [`read_working_file`], for image/binary handling.
pub fn read_working_file_bytes(
    repo_path: &Path,
    rel_path: &str,
) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(repo_path.join(rel_path)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Whether a working-tree file looks binary — a NUL byte in the first 8 KiB
/// (git's heuristic). Reads only that prefix, so a large binary isn't slurped in
/// full just to be classified. Absent or unreadable ⇒ `false`.
pub fn working_file_is_binary(repo_path: &Path, rel_path: &str) -> bool {
    use std::io::Read;
    let Ok(f) = std::fs::File::open(repo_path.join(rel_path)) else {
        return false;
    };
    // Read up to 8 KiB reliably — a single `read` may short-read, so `take` +
    // `read_to_end` (which loops to the limit or EOF) is used rather than one
    // `read` into a fixed buffer.
    let mut buf = Vec::with_capacity(8192);
    if f.take(8192).read_to_end(&mut buf).is_err() {
        return false;
    }
    buf.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_present_file_and_none_for_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        assert_eq!(
            read_working_file(dir.path(), "a.txt").unwrap().as_deref(),
            Some("hello\n")
        );
        assert_eq!(read_working_file(dir.path(), "missing.txt").unwrap(), None);
    }

    #[test]
    fn bytes_variant_round_trips_and_none_for_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.bin"), [0u8, 1, 2, 255]).unwrap();
        assert_eq!(
            read_working_file_bytes(dir.path(), "b.bin").unwrap(),
            Some(vec![0, 1, 2, 255])
        );
        assert_eq!(read_working_file_bytes(dir.path(), "nope").unwrap(), None);
    }

    #[test]
    fn binary_detection_flags_nul_and_clears_text() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("t.txt"), "plain text\n").unwrap();
        std::fs::write(dir.path().join("b.bin"), [b'x', 0, b'y']).unwrap();
        assert!(!working_file_is_binary(dir.path(), "t.txt"));
        assert!(working_file_is_binary(dir.path(), "b.bin"));
        assert!(!working_file_is_binary(dir.path(), "absent"));
    }

    #[test]
    fn binary_detection_scans_the_full_8kib_window() {
        let dir = tempfile::tempdir().unwrap();
        // NUL near the end of the window must be found — guards against a
        // short-read only inspecting the first chunk.
        let mut data = vec![b'a'; 8000];
        data.push(0);
        std::fs::write(dir.path().join("mid.bin"), &data).unwrap();
        assert!(working_file_is_binary(dir.path(), "mid.bin"));
    }

    #[test]
    fn binary_detection_stops_at_8kib() {
        let dir = tempfile::tempdir().unwrap();
        // A NUL just past the 8 KiB window is not scanned (git's heuristic).
        let mut data = vec![b'a'; 8192];
        data.push(0);
        std::fs::write(dir.path().join("late.bin"), &data).unwrap();
        assert!(!working_file_is_binary(dir.path(), "late.bin"));
    }
}
