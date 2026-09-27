use std::path::PathBuf;

use crate::types::{FileChange, FileChangeKind};

/// Parse `git diff --name-status` output into structured [`FileChange`] values.
///
/// git produces tab-separated lines — status letter, then path(s), separated
/// by tabs. Examples: `M<TAB>path.rs`, `R100<TAB>old.rs<TAB>new.rs`.
///
/// The leading status letter may be followed by a similarity score
/// (e.g. `R100`, `C75`); the score is recognized but not retained.
///
/// A path git quoted (with the default `core.quotePath`, any path holding a
/// control character, `"`, `\` or a non-ASCII byte: `"\303\251.rs"`) is unquoted.
///
/// Unknown status letters are skipped. Blank lines are skipped.
pub fn parse_git_diff_name_status(output: &str) -> Vec<FileChange> {
    let mut changes = Vec::new();
    for line in output.lines() {
        // Not trimmed: a path may end in whitespace, and `lines()` already drops `\r\n`.
        if line.trim().is_empty() {
            continue;
        }

        let mut parts = line.split('\t');
        let Some(status) = parts.next() else { continue };

        let kind = match status.chars().next() {
            Some('M') => FileChangeKind::Modified,
            Some('A') => FileChangeKind::Added,
            Some('D') => FileChangeKind::Deleted,
            Some('R') => FileChangeKind::Renamed,
            Some('C') => FileChangeKind::Copied,
            _ => continue,
        };

        match kind {
            FileChangeKind::Renamed | FileChangeKind::Copied => {
                let (Some(from), Some(to)) = (parts.next(), parts.next()) else {
                    continue;
                };
                changes.push(FileChange {
                    kind,
                    path: git_path(to),
                    from_path: Some(git_path(from)),
                });
            }
            _ => {
                let Some(path) = parts.next() else { continue };
                changes.push(FileChange {
                    kind,
                    path: git_path(path),
                    from_path: None,
                });
            }
        }
    }
    changes
}

/// A path field as git printed it: unquoted if git quoted it, else as is. git always
/// quotes a path containing `"`, so a field that starts with one was quoted. One that
/// does not unquote cleanly is kept as printed rather than dropped.
fn git_path(field: &str) -> PathBuf {
    match c_unquote(field) {
        Some(bytes) => path_from_bytes(bytes),
        None => PathBuf::from(field),
    }
}

/// Undo git's C-style quoting (`quote.c`): named escapes plus three-digit octal bytes.
fn c_unquote(field: &str) -> Option<Vec<u8>> {
    let inner = field.strip_prefix('"')?.strip_suffix('"')?.as_bytes();
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        if inner[i] != b'\\' {
            out.push(inner[i]);
            i += 1;
            continue;
        }
        let escaped = *inner.get(i + 1)?;
        let byte = match escaped {
            b'a' => 0x07,
            b'b' => 0x08,
            b't' => b'\t',
            b'n' => b'\n',
            b'v' => 0x0b,
            b'f' => 0x0c,
            b'r' => b'\r',
            b'"' | b'\\' => escaped,
            b'0'..=b'3' => {
                let digits = std::str::from_utf8(inner.get(i + 1..i + 4)?).ok()?;
                out.push(u8::from_str_radix(digits, 8).ok()?);
                i += 4;
                continue;
            }
            _ => return None,
        };
        out.push(byte);
        i += 2;
    }
    Some(out)
}

// One function with cfg'd bodies rather than two cfg'd functions: cargo-mutants mutates
// source text, so a separate non-unix function yields a mutant no unix test build compiles,
// which reads as MISSED.
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert!(parse_git_diff_name_status("").is_empty());
    }

    #[test]
    fn modified() {
        let changes = parse_git_diff_name_status("M\tsrc/lib.rs");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, FileChangeKind::Modified);
        assert_eq!(changes[0].path, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn added() {
        let changes = parse_git_diff_name_status("A\tnew.rs");
        assert_eq!(changes[0].kind, FileChangeKind::Added);
    }

    #[test]
    fn deleted() {
        let changes = parse_git_diff_name_status("D\told.rs");
        assert_eq!(changes[0].kind, FileChangeKind::Deleted);
    }

    #[test]
    fn renamed_with_similarity_score() {
        let changes = parse_git_diff_name_status("R100\told.rs\tnew.rs");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, FileChangeKind::Renamed);
        assert_eq!(changes[0].path, PathBuf::from("new.rs"));
        assert_eq!(changes[0].from_path, Some(PathBuf::from("old.rs")));
    }

    #[test]
    fn copied_with_similarity_score() {
        let changes = parse_git_diff_name_status("C75\tfrom.rs\tto.rs");
        assert_eq!(changes[0].kind, FileChangeKind::Copied);
        assert_eq!(changes[0].from_path, Some(PathBuf::from("from.rs")));
        assert_eq!(changes[0].path, PathBuf::from("to.rs"));
    }

    #[test]
    fn multiple_lines() {
        let output = "M\ta.rs\nA\tb.rs\nD\tc.rs\nR100\told.rs\tnew.rs";
        let changes = parse_git_diff_name_status(output);
        assert_eq!(changes.len(), 4);
        assert_eq!(changes[0].kind, FileChangeKind::Modified);
        assert_eq!(changes[3].kind, FileChangeKind::Renamed);
    }

    #[test]
    fn skips_blank_lines() {
        let changes = parse_git_diff_name_status("\nM\ta.rs\n\nA\tb.rs\n");
        assert_eq!(changes.len(), 2);
    }

    #[test]
    fn skips_unknown_status() {
        let changes = parse_git_diff_name_status("X\tfoo.rs\nM\tbar.rs");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, PathBuf::from("bar.rs"));
    }

    #[test]
    fn rename_without_second_path_skipped() {
        let changes = parse_git_diff_name_status("R100\tjust_one.rs");
        assert!(changes.is_empty());
    }

    #[test]
    fn quoted_paths_are_unquoted() {
        // git's default core.quotePath: found by the diff_summary_roundtrip fuzz target.
        let changes = parse_git_diff_name_status("A\t\"\\303\\251.rs\"\nC75\t\"tab\\there\"\t\"q\\\"\\\\\\n\"");
        assert_eq!(changes[0].path, PathBuf::from("é.rs"));
        assert_eq!(changes[1].from_path, Some(PathBuf::from("tab\there")));
        assert_eq!(changes[1].path, PathBuf::from("q\"\\\n"));
    }

    #[test]
    fn every_named_escape_and_the_octal_bounds_unquote() {
        let changes = parse_git_diff_name_status("M\t\"\\a\\b\\v\\f\\r\\000\\177\\377\"");
        let want: &[u8] = &[0x07, 0x08, 0x0b, 0x0c, b'\r', 0x00, 0x7f, 0xff];
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            assert_eq!(changes[0].path.as_os_str().as_bytes(), want);
        }
        #[cfg(not(unix))]
        assert_eq!(changes[0].path, PathBuf::from(String::from_utf8_lossy(want).into_owned()));
    }

    #[test]
    fn a_malformed_quoted_path_is_kept_as_printed() {
        let changes = parse_git_diff_name_status("M\t\"bad\\q\"\nM\t\"\\30\"\nM\t\"");
        assert_eq!(changes[0].path, PathBuf::from("\"bad\\q\""));
        assert_eq!(changes[1].path, PathBuf::from("\"\\30\""));
        assert_eq!(changes[2].path, PathBuf::from("\""));
    }

    #[test]
    fn trailing_whitespace_in_a_path_is_kept() {
        assert_eq!(parse_git_diff_name_status("M\ttrail \n")[0].path, PathBuf::from("trail "));
    }

    #[test]
    fn path_with_spaces_preserved() {
        let changes = parse_git_diff_name_status("M\tpath with spaces.rs");
        assert_eq!(changes[0].path, PathBuf::from("path with spaces.rs"));
    }
}
