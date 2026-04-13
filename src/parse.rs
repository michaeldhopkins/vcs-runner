use std::collections::HashSet;

use serde::Deserialize;

use crate::types::{
    Bookmark, ConflictState, ContentState, GitRemote, LogEntry, RemoteStatus, WorkingCopy,
};

/// jj template for `jj bookmark list` producing line-delimited JSON.
///
/// Use with [`parse_bookmark_output`] to get structured [`Bookmark`] values.
/// jj's `escape_json()` includes surrounding quotes, so array elements
/// use it directly with comma joins (no extra quote wrapping).
pub const BOOKMARK_TEMPLATE: &str = concat!(
    r#"'{"name":' ++ name.escape_json()"#,
    r#" ++ ',"commitId":' ++ normal_target.commit_id().short().escape_json()"#,
    r#" ++ ',"changeId":' ++ normal_target.change_id().short().escape_json()"#,
    r#" ++ ',"localBookmarks":[' ++ normal_target.local_bookmarks().map(|b| b.name().escape_json()).join(',') ++ ']'"#,
    r#" ++ ',"remoteBookmarks":[' ++ normal_target.remote_bookmarks().map(|b| stringify(b.name() ++ "@" ++ b.remote()).escape_json()).join(',') ++ ']'"#,
    r#" ++ '}' ++ "\n""#,
);

/// jj template for `jj log` producing line-delimited JSON entries.
///
/// Use with [`parse_log_output`] to get structured [`LogEntry`] values.
pub const LOG_TEMPLATE: &str = concat!(
    r#"'{"commitId":' ++ commit_id.short().escape_json()"#,
    r#" ++ ',"changeId":' ++ change_id.short().escape_json()"#,
    r#" ++ ',"authorName":' ++ author.name().escape_json()"#,
    r#" ++ ',"authorEmail":' ++ stringify(author.email()).escape_json()"#,
    r#" ++ ',"description":' ++ description.escape_json()"#,
    r#" ++ ',"parents":[' ++ parents.map(|p| p.commit_id().short().escape_json()).join(',') ++ ']'"#,
    r#" ++ ',"localBookmarks":[' ++ local_bookmarks.map(|b| b.name().escape_json()).join(',') ++ ']'"#,
    r#" ++ ',"remoteBookmarks":[' ++ remote_bookmarks.map(|b| stringify(b.name() ++ "@" ++ b.remote()).escape_json()).join(',') ++ ']'"#,
    r#" ++ ',"isWorkingCopy":' ++ if(current_working_copy, '"true"', '"false"')"#,
    r#" ++ ',"conflict":' ++ if(conflict, '"true"', '"false"')"#,
    r#" ++ ',"empty":' ++ if(empty, '"true"', '"false"')"#,
    r#" ++ '}' ++ "\n""#,
);

/// Result of parsing bookmark output, including any skipped entries.
#[derive(Debug)]
pub struct BookmarkParseResult {
    pub bookmarks: Vec<Bookmark>,
    /// Names of bookmarks that were skipped due to malformed JSON
    /// (typically stale/conflicted bookmarks pointing at missing commits).
    pub skipped: Vec<String>,
}

/// Result of parsing log output, including any skipped entries.
#[derive(Debug)]
pub struct LogParseResult {
    pub entries: Vec<LogEntry>,
    /// Lines that failed to parse.
    pub skipped: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBookmark {
    name: String,
    commit_id: String,
    change_id: String,
    local_bookmarks: Vec<String>,
    remote_bookmarks: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLogEntry {
    commit_id: String,
    change_id: String,
    author_name: String,
    author_email: String,
    description: String,
    parents: Vec<String>,
    local_bookmarks: Vec<String>,
    remote_bookmarks: Vec<String>,
    is_working_copy: String,
    conflict: String,
    empty: String,
}

fn extract_name_from_malformed_json(line: &str) -> Option<String> {
    let after_key = line.split(r#""name":"#).nth(1)?;
    let after_quote = after_key.strip_prefix('"')?;
    let end = after_quote.find('"')?;
    Some(after_quote[..end].to_string())
}

fn resolve_remote_status(name: &str, remote_bookmarks: &[String]) -> RemoteStatus {
    let non_git_remotes: Vec<&String> = remote_bookmarks
        .iter()
        .filter(|rb| !rb.is_empty() && !rb.ends_with("@git"))
        .collect();

    if non_git_remotes.is_empty() {
        return RemoteStatus::Local;
    }

    let synced = non_git_remotes
        .iter()
        .any(|rb| rb.starts_with(&format!("{name}@")));

    if synced { RemoteStatus::Synced } else { RemoteStatus::Unsynced }
}

/// Parse `jj bookmark list --template BOOKMARK_TEMPLATE` output.
///
/// Skips conflicted/stale bookmarks that produce unparseable JSON and
/// filters out remote-only entries (empty `localBookmarks`).
pub fn parse_bookmark_output(output: &str) -> BookmarkParseResult {
    let mut bookmarks = Vec::new();
    let mut skipped = Vec::new();
    let mut warned_names: HashSet<String> = HashSet::new();

    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let raw: RawBookmark = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => {
                let name = extract_name_from_malformed_json(line)
                    .unwrap_or_else(|| "<unknown>".to_string());
                if warned_names.insert(name.clone()) {
                    skipped.push(name);
                }
                continue;
            }
        };

        if raw.local_bookmarks.is_empty() {
            continue;
        }

        let remote = resolve_remote_status(&raw.name, &raw.remote_bookmarks);

        bookmarks.push(Bookmark {
            name: raw.name,
            commit_id: raw.commit_id,
            change_id: raw.change_id,
            remote,
        });
    }

    BookmarkParseResult { bookmarks, skipped }
}

/// Parse `jj log --template LOG_TEMPLATE` output.
///
/// Skips malformed lines rather than failing, returning them in
/// `LogParseResult::skipped` for the caller to handle.
pub fn parse_log_output(output: &str) -> LogParseResult {
    let mut entries = Vec::new();
    let mut skipped = Vec::new();

    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let raw: RawLogEntry = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                skipped.push(format!("{e}: {line}"));
                continue;
            }
        };

        let working_copy = if raw.is_working_copy == "true" {
            WorkingCopy::Current
        } else {
            WorkingCopy::Background
        };
        let conflict = if raw.conflict == "true" {
            ConflictState::Conflicted
        } else {
            ConflictState::Clean
        };
        let content = if raw.empty == "true" {
            ContentState::Empty
        } else {
            ContentState::HasContent
        };

        entries.push(LogEntry {
            commit_id: raw.commit_id,
            change_id: raw.change_id,
            author_name: raw.author_name,
            author_email: raw.author_email,
            description: raw.description,
            parents: raw.parents.into_iter().filter(|p| !p.is_empty()).collect(),
            local_bookmarks: raw
                .local_bookmarks
                .into_iter()
                .filter(|b| !b.is_empty())
                .collect(),
            remote_bookmarks: raw
                .remote_bookmarks
                .into_iter()
                .filter(|b| !b.is_empty())
                .collect(),
            working_copy,
            conflict,
            content,
        });
    }

    LogParseResult { entries, skipped }
}

/// Parse `jj git remote list` output into `GitRemote` values.
pub fn parse_remote_list(output: &str) -> Vec<GitRemote> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let name = parts.next()?.trim().to_string();
            let url = parts.next()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            Some(GitRemote { name, url })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_name_from_malformed_json ---

    #[test]
    fn extract_name_valid() {
        let line = r#"{"name":"feat/stale","commitId":<Error: No Commit available>}"#;
        assert_eq!(extract_name_from_malformed_json(line), Some("feat/stale".to_string()));
    }

    #[test]
    fn extract_name_garbage() {
        assert_eq!(extract_name_from_malformed_json("garbage"), None);
    }

    #[test]
    fn extract_name_empty_string() {
        assert_eq!(extract_name_from_malformed_json(""), None);
    }

    #[test]
    fn extract_name_no_value() {
        assert_eq!(extract_name_from_malformed_json(r#"{"name":}"#), None);
    }

    #[test]
    fn extract_name_with_slash() {
        let line = r#"{"name":"feat/deep/nested","commitId":"abc"}"#;
        assert_eq!(extract_name_from_malformed_json(line), Some("feat/deep/nested".to_string()));
    }

    // --- resolve_remote_status ---

    #[test]
    fn remote_status_no_remotes() {
        assert_eq!(resolve_remote_status("feat", &[]), RemoteStatus::Local);
    }

    #[test]
    fn remote_status_git_only() {
        assert_eq!(resolve_remote_status("feat", &["feat@git".into()]), RemoteStatus::Local);
    }

    #[test]
    fn remote_status_empty_strings() {
        assert_eq!(resolve_remote_status("feat", &["".into(), "".into()]), RemoteStatus::Local);
    }

    #[test]
    fn remote_status_synced() {
        assert_eq!(resolve_remote_status("feat", &["feat@origin".into()]), RemoteStatus::Synced);
    }

    #[test]
    fn remote_status_synced_multiple() {
        let remotes = vec!["feat@origin".into(), "feat@upstream".into()];
        assert_eq!(resolve_remote_status("feat", &remotes), RemoteStatus::Synced);
    }

    #[test]
    fn remote_status_unsynced() {
        assert_eq!(resolve_remote_status("feat", &["other@origin".into()]), RemoteStatus::Unsynced);
    }

    #[test]
    fn remote_status_git_ignored_for_sync() {
        let remotes = vec!["feat@git".into(), "other@origin".into()];
        assert_eq!(resolve_remote_status("feat", &remotes), RemoteStatus::Unsynced);
    }

    // --- parse_bookmark_output ---

    #[test]
    fn bookmark_empty_output() {
        let result = parse_bookmark_output("");
        assert!(result.bookmarks.is_empty());
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn bookmark_no_remote() {
        let output = r#"{"name":"feature","commitId":"abc123","changeId":"xyz789","localBookmarks":["feature"],"remoteBookmarks":[]}"#;
        let result = parse_bookmark_output(output);
        assert_eq!(result.bookmarks.len(), 1);
        assert_eq!(result.bookmarks[0].name, "feature");
        assert_eq!(result.bookmarks[0].remote, RemoteStatus::Local);
    }

    #[test]
    fn bookmark_with_synced_remote() {
        let output = r#"{"name":"feature","commitId":"abc123","changeId":"xyz789","localBookmarks":["feature"],"remoteBookmarks":["feature@origin"]}"#;
        let result = parse_bookmark_output(output);
        assert_eq!(result.bookmarks[0].remote, RemoteStatus::Synced);
    }

    #[test]
    fn bookmark_with_unsynced_remote() {
        let output = r#"{"name":"feature","commitId":"abc123","changeId":"xyz789","localBookmarks":["feature"],"remoteBookmarks":["other@origin"]}"#;
        let result = parse_bookmark_output(output);
        assert_eq!(result.bookmarks[0].remote, RemoteStatus::Unsynced);
    }

    #[test]
    fn bookmark_git_remote_excluded() {
        let output = r#"{"name":"feature","commitId":"abc123","changeId":"xyz789","localBookmarks":["feature"],"remoteBookmarks":["feature@git"]}"#;
        let result = parse_bookmark_output(output);
        assert_eq!(result.bookmarks[0].remote, RemoteStatus::Local);
    }

    #[test]
    fn bookmark_conflicted_skipped_with_name() {
        let output = concat!(
            r#"{"name":"feat/stale","commitId":<Error: No Commit available>,"changeId":<Error: No Commit available>,"localBookmarks":[<Error: No Commit available>],"remoteBookmarks":[<Error: No Commit available>]}"#,
            "\n",
            r#"{"name":"feat/good","commitId":"abc123","changeId":"xyz789","localBookmarks":["feat/good"],"remoteBookmarks":["feat/good@origin"]}"#,
            "\n",
        );
        let result = parse_bookmark_output(output);
        assert_eq!(result.bookmarks.len(), 1);
        assert_eq!(result.bookmarks[0].name, "feat/good");
        assert_eq!(result.skipped, vec!["feat/stale"]);
    }

    #[test]
    fn bookmark_completely_unparseable() {
        let result = parse_bookmark_output("not json at all");
        assert!(result.bookmarks.is_empty());
        assert_eq!(result.skipped, vec!["<unknown>"]);
    }

    // --- parse_log_output ---

    #[test]
    fn log_empty_output() {
        let result = parse_log_output("");
        assert!(result.entries.is_empty());
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn log_basic_entry() {
        let output = r#"{"commitId":"abc123","changeId":"xyz789","authorName":"Alice","authorEmail":"alice@example.com","description":"Add feature\n\nDetailed","parents":["def456"],"localBookmarks":["feature"],"remoteBookmarks":[],"isWorkingCopy":"false","conflict":"false","empty":"false"}"#;
        let result = parse_log_output(output);
        assert_eq!(result.entries.len(), 1);
        let entry = &result.entries[0];
        assert_eq!(entry.commit_id, "abc123");
        assert_eq!(entry.summary(), "Add feature");
        assert_eq!(entry.parents, vec!["def456"]);
        assert_eq!(entry.working_copy, WorkingCopy::Background);
        assert_eq!(entry.conflict, ConflictState::Clean);
        assert_eq!(entry.content, ContentState::HasContent);
    }

    #[test]
    fn log_empty_commit() {
        let output = r#"{"commitId":"abc","changeId":"xyz","authorName":"A","authorEmail":"a@b","description":"empty","parents":["p1"],"localBookmarks":[],"remoteBookmarks":[],"isWorkingCopy":"false","conflict":"false","empty":"true"}"#;
        let result = parse_log_output(output);
        assert!(result.entries[0].content.is_empty());
        assert!(!result.entries[0].conflict.is_conflicted());
    }

    #[test]
    fn log_conflicted_commit() {
        let output = r#"{"commitId":"abc","changeId":"xyz","authorName":"A","authorEmail":"a@b","description":"conflict","parents":["p1"],"localBookmarks":[],"remoteBookmarks":[],"isWorkingCopy":"false","conflict":"true","empty":"false"}"#;
        let result = parse_log_output(output);
        assert!(result.entries[0].conflict.is_conflicted());
        assert!(!result.entries[0].content.is_empty());
    }

    #[test]
    fn log_conflicted_and_empty() {
        let output = r#"{"commitId":"abc","changeId":"xyz","authorName":"A","authorEmail":"a@b","description":"both","parents":["p1"],"localBookmarks":[],"remoteBookmarks":[],"isWorkingCopy":"false","conflict":"true","empty":"true"}"#;
        let result = parse_log_output(output);
        assert!(result.entries[0].conflict.is_conflicted());
        assert!(result.entries[0].content.is_empty());
    }

    #[test]
    fn log_working_copy() {
        let output = r#"{"commitId":"abc","changeId":"xyz","authorName":"A","authorEmail":"a@b","description":"wip","parents":["p1"],"localBookmarks":[],"remoteBookmarks":[],"isWorkingCopy":"true","conflict":"false","empty":"false"}"#;
        let result = parse_log_output(output);
        assert_eq!(result.entries[0].working_copy, WorkingCopy::Current);
    }

    #[test]
    fn log_malformed_line_skipped() {
        let output = concat!(
            "not json\n",
            r#"{"commitId":"abc","changeId":"xyz","authorName":"A","authorEmail":"a@b","description":"ok","parents":[],"localBookmarks":[],"remoteBookmarks":[],"isWorkingCopy":"false","conflict":"false","empty":"false"}"#,
            "\n",
        );
        let result = parse_log_output(output);
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.skipped.len(), 1);
    }

    #[test]
    fn log_summary_method() {
        let output = r#"{"commitId":"abc","changeId":"xyz","authorName":"A","authorEmail":"a@b","description":"line1\nline2\nline3","parents":[],"localBookmarks":[],"remoteBookmarks":[],"isWorkingCopy":"false","conflict":"false","empty":"false"}"#;
        let result = parse_log_output(output);
        assert_eq!(result.entries[0].summary(), "line1");
        assert_eq!(result.entries[0].description, "line1\nline2\nline3");
    }

    // --- parse_remote_list ---

    #[test]
    fn remote_list_empty() {
        assert!(parse_remote_list("").is_empty());
    }

    #[test]
    fn remote_list_single() {
        let remotes = parse_remote_list("origin https://github.com/user/repo.git");
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(remotes[0].url, "https://github.com/user/repo.git");
    }

    #[test]
    fn remote_list_multiple() {
        let remotes = parse_remote_list("origin https://a.com\nupstream https://b.com");
        assert_eq!(remotes.len(), 2);
    }

    #[test]
    fn remote_list_skips_empty_lines() {
        let remotes = parse_remote_list("\norigin https://example.com\n\n");
        assert_eq!(remotes.len(), 1);
    }

    // --- template constants ---

    #[test]
    fn bookmark_template_contains_required_fields() {
        assert!(BOOKMARK_TEMPLATE.contains("name"));
        assert!(BOOKMARK_TEMPLATE.contains("commitId"));
        assert!(BOOKMARK_TEMPLATE.contains("escape_json"));
    }

    #[test]
    fn log_template_contains_required_fields() {
        assert!(LOG_TEMPLATE.contains("commitId"));
        assert!(LOG_TEMPLATE.contains("description"));
        assert!(LOG_TEMPLATE.contains("conflict"));
    }
}
