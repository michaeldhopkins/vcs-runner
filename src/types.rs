/// Whether a jj commit is the current working copy.
#[cfg(feature = "jj-parse")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WorkingCopy {
    #[default]
    Background,
    Current,
}

/// Whether a jj commit has unresolved conflicts.
#[cfg(feature = "jj-parse")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConflictState {
    #[default]
    Clean,
    Conflicted,
}

#[cfg(feature = "jj-parse")]
impl ConflictState {
    pub fn is_conflicted(self) -> bool {
        matches!(self, Self::Conflicted)
    }
}

/// Whether a jj commit is empty (no file changes).
#[cfg(feature = "jj-parse")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ContentState {
    #[default]
    HasContent,
    Empty,
}

#[cfg(feature = "jj-parse")]
impl ContentState {
    pub fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }
}

/// A jj log entry parsed from jj template JSON output.
///
/// Not directly deserializable from jj's JSON — use [`crate::parse_log_output`]
/// which handles the raw string booleans and populates the enum fields.
#[cfg(feature = "jj-parse")]
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub commit_id: String,
    pub change_id: String,
    pub author_name: String,
    pub author_email: String,
    pub description: String,
    pub parents: Vec<String>,
    pub local_bookmarks: Vec<String>,
    pub remote_bookmarks: Vec<String>,
    pub working_copy: WorkingCopy,
    pub conflict: ConflictState,
    pub content: ContentState,
}

#[cfg(feature = "jj-parse")]
impl LogEntry {
    /// The first line of the commit description.
    pub fn summary(&self) -> &str {
        self.description.lines().next().unwrap_or("")
    }
}

/// Sync status of a bookmark relative to its remote.
#[cfg(feature = "jj-parse")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemoteStatus {
    /// Bookmark exists only locally (no non-git remote tracking branch).
    Local,
    /// Remote exists but local and remote have diverged.
    Unsynced,
    /// Local and remote point to the same commit.
    Synced,
}

#[cfg(feature = "jj-parse")]
impl RemoteStatus {
    pub fn has_remote(self) -> bool {
        !matches!(self, Self::Local)
    }

    pub fn is_synced(self) -> bool {
        matches!(self, Self::Synced)
    }
}

/// A jj bookmark with sync status.
#[cfg(feature = "jj-parse")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub name: String,
    pub commit_id: String,
    pub change_id: String,
    pub remote: RemoteStatus,
}

/// A git remote.
#[cfg(feature = "jj-parse")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRemote {
    pub name: String,
    pub url: String,
}

/// The kind of change a file underwent in a diff.
#[cfg(any(feature = "jj-parse", feature = "git-parse"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
}

/// A single file change in a diff summary.
///
/// `from_path` is populated only for `Renamed` and `Copied` — it records
/// the previous/source path. `path` is always the resulting path.
#[cfg(any(feature = "jj-parse", feature = "git-parse"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub kind: FileChangeKind,
    pub path: std::path::PathBuf,
    pub from_path: Option<std::path::PathBuf>,
}

#[cfg(all(test, feature = "jj-parse"))]
mod tests {
    use super::*;

    #[test]
    fn conflict_state() {
        assert!(!ConflictState::Clean.is_conflicted());
        assert!(ConflictState::Conflicted.is_conflicted());
    }

    #[test]
    fn content_state() {
        assert!(!ContentState::HasContent.is_empty());
        assert!(ContentState::Empty.is_empty());
    }

    #[test]
    fn working_copy_default() {
        assert_eq!(WorkingCopy::default(), WorkingCopy::Background);
    }

    #[test]
    fn remote_status_local() {
        assert!(!RemoteStatus::Local.has_remote());
        assert!(!RemoteStatus::Local.is_synced());
    }

    #[test]
    fn remote_status_unsynced() {
        assert!(RemoteStatus::Unsynced.has_remote());
        assert!(!RemoteStatus::Unsynced.is_synced());
    }

    #[test]
    fn remote_status_synced() {
        assert!(RemoteStatus::Synced.has_remote());
        assert!(RemoteStatus::Synced.is_synced());
    }

    #[test]
    fn log_entry_summary() {
        let entry = LogEntry {
            commit_id: "abc".into(),
            change_id: "xyz".into(),
            author_name: "A".into(),
            author_email: "a@b".into(),
            description: "first line\nsecond line".into(),
            parents: vec![],
            local_bookmarks: vec![],
            remote_bookmarks: vec![],
            working_copy: WorkingCopy::Background,
            conflict: ConflictState::Clean,
            content: ContentState::HasContent,
        };
        assert_eq!(entry.summary(), "first line");
    }

    #[test]
    fn log_entry_summary_empty_description() {
        let entry = LogEntry {
            commit_id: "abc".into(),
            change_id: "xyz".into(),
            author_name: "A".into(),
            author_email: "a@b".into(),
            description: String::new(),
            parents: vec![],
            local_bookmarks: vec![],
            remote_bookmarks: vec![],
            working_copy: WorkingCopy::Background,
            conflict: ConflictState::Clean,
            content: ContentState::HasContent,
        };
        assert_eq!(entry.summary(), "");
    }
}
