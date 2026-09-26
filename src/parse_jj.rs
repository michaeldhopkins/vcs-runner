use std::collections::HashSet;
use std::path::PathBuf;

use serde::Deserialize;

use crate::types::{
    Bookmark, ConflictState, ContentState, FileChange, FileChangeKind, GitRemote, LogEntry,
    RemoteStatus, WorkingCopy,
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
    // The name is a whole JSON string even when what follows it is not JSON, so decode
    // just that one value: escape_json() escapes quotes, backslashes and control
    // characters, which slicing between quotes would get wrong.
    serde_json::Deserializer::from_str(after_key)
        .into_iter::<String>()
        .next()?
        .ok()
}

fn resolve_remote_status(name: &str, remote_bookmarks: &[String]) -> RemoteStatus {
    let non_git_remotes: Vec<&String> = remote_bookmarks
        .iter()
        .filter(|rb| !rb.is_empty() && !rb.ends_with("@git"))
        .collect();

    if non_git_remotes.is_empty() {
        return RemoteStatus::Local;
    }

    // A remote bookmark renders as `name@remote`. Bookmark names may contain `@`
    // (`feat@v2`), remote names in practice do not, so the owner is everything before
    // the last `@`; a prefix match would credit `feat@v2@origin` to `feat`.
    let synced = non_git_remotes
        .iter()
        .any(|rb| rb.rsplit_once('@').is_some_and(|(owner, _)| owner == name));

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

/// The two paths of a rename or copy in `jj diff --summary`.
fn split_rename(rest: &str) -> Option<(String, String)> {
    expand_brace_rename(rest).or_else(|| {
        rest.split_once(" -> ").map(|(from, to)| (from.to_string(), to.to_string()))
    })
}

/// `prefix/{old => new}/suffix`, where the braces open at the start or after a `/` and
/// close at the end or before one. Either side of ` => ` may be empty, as in
/// `g/{e => }/x.rs`, and then the slashes around it collapse to one.
fn expand_brace_rename(s: &str) -> Option<(String, String)> {
    let open = s
        .char_indices()
        .find(|&(i, c)| c == '{' && (i == 0 || s[..i].ends_with('/')))?
        .0;
    let (prefix, tail) = (&s[..open], &s[open + 1..]);
    let close = tail
        .char_indices()
        .rev()
        .find(|&(i, c)| c == '}' && (i + 1 == tail.len() || tail[i + 1..].starts_with('/')))?
        .0;
    let (inner, suffix) = (&tail[..close], &tail[close + 1..]);
    let (old, new) = inner.split_once(" => ")?;
    Some((join_rename_side(prefix, old, suffix), join_rename_side(prefix, new, suffix)))
}

fn join_rename_side(prefix: &str, middle: &str, suffix: &str) -> String {
    let prefix = prefix.strip_suffix('/').unwrap_or(prefix);
    let suffix = suffix.strip_prefix('/').unwrap_or(suffix);
    [prefix, middle, suffix].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("/")
}

/// Parse `jj diff --summary` output into structured [`FileChange`] values.
///
/// jj produces lines like:
/// ```text
/// M path/to/file.rs
/// A new_file.rs
/// D removed.rs
/// R src/{old.rs => new.rs}
/// C {from.rs => to.rs}
/// R old/path.rs -> new/path.rs
/// ```
///
/// A rename or copy is jj's brace form, in which the path components the two sides
/// share are written once outside the braces (`R {d => g}/e/x.rs` is `d/e/x.rs` to
/// `g/e/x.rs`), or the older `old -> new`. jj does not quote paths, so a path that
/// itself contains `{`, `}` or ` => ` makes a rename line ambiguous.
///
/// Unknown status letters are skipped. Blank lines are skipped.
pub fn parse_diff_summary(output: &str) -> Vec<FileChange> {
    let mut changes = Vec::new();
    for line in output.lines() {
        // Not trimmed: a path may end in whitespace, and `lines()` already drops `\r\n`.
        if line.trim().is_empty() {
            continue;
        }
        let Some((kind_str, rest)) = line.split_once(' ') else {
            continue;
        };
        let kind = match kind_str {
            "M" => FileChangeKind::Modified,
            "A" => FileChangeKind::Added,
            "D" => FileChangeKind::Deleted,
            "R" => FileChangeKind::Renamed,
            "C" => FileChangeKind::Copied,
            _ => continue,
        };

        match kind {
            FileChangeKind::Renamed | FileChangeKind::Copied => {
                if let Some((from, to)) = split_rename(rest) {
                    changes.push(FileChange {
                        kind,
                        path: PathBuf::from(to),
                        from_path: Some(PathBuf::from(from)),
                    });
                }
            }
            _ => changes.push(FileChange {
                kind,
                path: PathBuf::from(rest),
                from_path: None,
            }),
        }
    }
    changes
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
    fn extract_name_decodes_json_escapes() {
        // Found by the bookmark_roundtrip fuzz target: jj's escape_json() escapes the
        // name, so a stale bookmark's name must be decoded, not sliced between quotes.
        let line = r#"{"name":"a\"b\\c\u0005","commitId":<Error: No Commit available>}"#;
        assert_eq!(extract_name_from_malformed_json(line), Some("a\"b\\c\u{5}".to_string()));
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
    fn remote_status_reads_the_owner_before_the_last_at() {
        // Found by the bookmark_roundtrip fuzz target: `feat@v2` is a legal bookmark
        // name, and its remote bookmark `feat@v2@origin` is not `feat`'s.
        assert_eq!(resolve_remote_status("feat", &["feat@v2@origin".into()]), RemoteStatus::Unsynced);
        assert_eq!(resolve_remote_status("feat@v2", &["feat@v2@origin".into()]), RemoteStatus::Synced);
        assert_eq!(resolve_remote_status("", &["@x@origin".into()]), RemoteStatus::Unsynced);
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

    // --- parse_diff_summary ---

    #[test]
    fn diff_summary_empty() {
        assert!(parse_diff_summary("").is_empty());
    }

    #[test]
    fn diff_summary_modified() {
        let changes = parse_diff_summary("M src/lib.rs");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, FileChangeKind::Modified);
        assert_eq!(changes[0].path, PathBuf::from("src/lib.rs"));
        assert_eq!(changes[0].from_path, None);
    }

    #[test]
    fn diff_summary_added() {
        let changes = parse_diff_summary("A new_file.rs");
        assert_eq!(changes[0].kind, FileChangeKind::Added);
        assert_eq!(changes[0].path, PathBuf::from("new_file.rs"));
    }

    #[test]
    fn diff_summary_deleted() {
        let changes = parse_diff_summary("D old.rs");
        assert_eq!(changes[0].kind, FileChangeKind::Deleted);
        assert_eq!(changes[0].path, PathBuf::from("old.rs"));
    }

    #[test]
    fn diff_summary_renamed() {
        let changes = parse_diff_summary("R old/path.rs -> new/path.rs");
        assert_eq!(changes[0].kind, FileChangeKind::Renamed);
        assert_eq!(changes[0].path, PathBuf::from("new/path.rs"));
        assert_eq!(changes[0].from_path, Some(PathBuf::from("old/path.rs")));
    }

    #[test]
    fn diff_summary_copied() {
        let changes = parse_diff_summary("C src/a.rs -> src/b.rs");
        assert_eq!(changes[0].kind, FileChangeKind::Copied);
        assert_eq!(changes[0].path, PathBuf::from("src/b.rs"));
        assert_eq!(changes[0].from_path, Some(PathBuf::from("src/a.rs")));
    }

    #[test]
    fn diff_summary_multiple() {
        let output = "M a.rs\nA b.rs\nD c.rs\nR old.rs -> new.rs";
        let changes = parse_diff_summary(output);
        assert_eq!(changes.len(), 4);
        assert_eq!(changes[0].kind, FileChangeKind::Modified);
        assert_eq!(changes[1].kind, FileChangeKind::Added);
        assert_eq!(changes[2].kind, FileChangeKind::Deleted);
        assert_eq!(changes[3].kind, FileChangeKind::Renamed);
    }

    #[test]
    fn diff_summary_skips_blank_lines() {
        let output = "\nM a.rs\n\nA b.rs\n\n";
        let changes = parse_diff_summary(output);
        assert_eq!(changes.len(), 2);
    }

    #[test]
    fn diff_summary_skips_unknown_status() {
        let output = "X mysterious.rs\nM known.rs";
        let changes = parse_diff_summary(output);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, PathBuf::from("known.rs"));
    }

    #[test]
    fn diff_summary_path_with_spaces() {
        let changes = parse_diff_summary("M path with spaces.rs");
        assert_eq!(changes[0].path, PathBuf::from("path with spaces.rs"));
    }

    fn pair_of(line: &str) -> (PathBuf, PathBuf) {
        let changes = parse_diff_summary(line);
        assert_eq!(changes.len(), 1, "{line}");
        (changes[0].from_path.clone().expect("a rename has a source"), changes[0].path.clone())
    }

    #[test]
    fn diff_summary_reads_jjs_brace_renames() {
        // Captured from `jj diff --summary` (jj 0.45); found by the
        // diff_summary_roundtrip fuzz target, which renders this form. Before, every
        // one of these was silently dropped.
        let cases = [
            ("R src/{a.rs => b.rs}", "src/a.rs", "src/b.rs"),
            ("R {d => g}/e/x.rs", "d/e/x.rs", "g/e/x.rs"),
            ("R {d/k.rs => k.rs}", "d/k.rs", "k.rs"),
            ("R {top.rs => moved.rs}", "top.rs", "moved.rs"),
            ("R d/e/{ => deeper}/p.rs", "d/e/p.rs", "d/e/deeper/p.rs"),
            ("R g/{e => }/x.rs", "g/e/x.rs", "g/x.rs"),
            ("C {f => f2}", "f", "f2"),
        ];
        for (line, from, to) in cases {
            assert_eq!(pair_of(line), (PathBuf::from(from), PathBuf::from(to)), "{line}");
        }
        assert_eq!(parse_diff_summary("C {f => f2}")[0].kind, FileChangeKind::Copied);
    }

    #[test]
    fn diff_summary_keeps_trailing_whitespace_in_a_path() {
        // `jj diff --summary` prints `M trail ` for a file named "trail ".
        assert_eq!(parse_diff_summary("M trail ")[0].path, PathBuf::from("trail "));
        assert_eq!(pair_of("R {a => b }"), (PathBuf::from("a"), PathBuf::from("b ")));
    }

    #[test]
    fn diff_summary_braces_in_a_modified_path_are_literal() {
        assert_eq!(parse_diff_summary("M {a => b}")[0].path, PathBuf::from("{a => b}"));
    }

    #[test]
    fn diff_summary_rename_without_arrow_skipped() {
        // "R" without " -> " is malformed and skipped
        let changes = parse_diff_summary("R just_one_path.rs");
        assert!(changes.is_empty());
    }
}
