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
/// Unknown status letters are skipped. Blank lines are skipped.
pub fn parse_git_diff_name_status(output: &str) -> Vec<FileChange> {
    let mut changes = Vec::new();
    for line in output.lines() {
        let line = line.trim_end();
        if line.is_empty() {
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
                    path: PathBuf::from(to),
                    from_path: Some(PathBuf::from(from)),
                });
            }
            _ => {
                let Some(path) = parts.next() else { continue };
                changes.push(FileChange {
                    kind,
                    path: PathBuf::from(path),
                    from_path: None,
                });
            }
        }
    }
    changes
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
    fn path_with_spaces_preserved() {
        let changes = parse_git_diff_name_status("M\tpath with spaces.rs");
        assert_eq!(changes[0].path, PathBuf::from("path with spaces.rs"));
    }
}
