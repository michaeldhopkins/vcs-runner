//! `jj bookmark list` with [`BOOKMARK_TEMPLATE`], and its parser.
//!
//! jj applies a bookmark-list template once per *ref*, not once per bookmark: a local
//! bookmark gets a line, and so does each of its tracked remote bookmarks whose target
//! differs from the local one, and a bookmark deleted locally but still on a remote
//! gets a line for the remote alone. A synced remote bookmark gets no line of its own.
//! So the template says which ref each line is (`remote`, null for a local one), and
//! the parser reports only local bookmarks, working out their sync state from the
//! remote lines and from the remote bookmarks on the local target. Verified against
//! jj 0.33 through 0.45 (`tests/fixtures/jj/`).

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::types::{Bookmark, RemoteStatus};

/// jj template for `jj bookmark list` producing line-delimited JSON.
///
/// Use with [`parse_bookmark_output`] to get structured [`Bookmark`] values.
/// jj's `escape_json()` includes surrounding quotes, so values use it directly.
/// `remoteRefs` lists the remote bookmarks on the target as `[name, remote]` pairs of
/// raw names; a `name@remote` string would carry jj's revset quoting (`"a@b"@origin`).
pub const BOOKMARK_TEMPLATE: &str = concat!(
    r#"'{"name":' ++ name.escape_json()"#,
    r#" ++ ',"remote":' ++ if(remote, remote.escape_json(), 'null')"#,
    r#" ++ ',"commitId":' ++ normal_target.commit_id().short().escape_json()"#,
    r#" ++ ',"changeId":' ++ normal_target.change_id().short().escape_json()"#,
    r#" ++ ',"localBookmarks":[' ++ normal_target.local_bookmarks().map(|b| b.name().escape_json()).join(',') ++ ']'"#,
    r#" ++ ',"remoteRefs":[' ++ normal_target.remote_bookmarks().map(|b| '[' ++ b.name().escape_json() ++ ',' ++ b.remote().escape_json() ++ ']').join(',') ++ ']'"#,
    r#" ++ '}' ++ "\n""#,
);

/// Result of parsing bookmark output, including any skipped entries.
#[derive(Debug)]
pub struct BookmarkParseResult {
    pub bookmarks: Vec<Bookmark>,
    /// Names of local bookmarks that were skipped due to malformed JSON: jj prints
    /// `<Error: No Commit available>` for a conflicted bookmark, or one deleted
    /// locally but still present on a remote.
    pub skipped: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBookmark {
    name: String,
    /// Absent in the pre-0.18 template's output, null for a local ref, the remote's
    /// name for a remote ref.
    #[serde(default, deserialize_with = "present")]
    remote: Option<serde_json::Value>,
    commit_id: String,
    change_id: String,
    local_bookmarks: Vec<String>,
    #[serde(default)]
    remote_refs: Vec<(String, String)>,
    /// The pre-0.18 template's `name@remote` strings.
    #[serde(default)]
    remote_bookmarks: Vec<String>,
}

/// `Some` whenever the key is present, `null` included: plain `Option` would read a
/// local ref's `"remote":null` the same as the older template's missing key.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(d).map(Some)
}

impl RawBookmark {
    /// `Some(remote)` for a remote ref's line, `None` for a local bookmark's.
    fn remote_name(&self) -> Option<&str> {
        match &self.remote {
            Some(serde_json::Value::String(remote)) => Some(remote),
            _ => None,
        }
    }
}

/// The name, and the remote if the line is a remote ref's, from a line that is not
/// JSON. The name and `remote` (which follows it) are whole JSON values even when what
/// comes after is jj's `<Error: …>` text, so each is decoded as one value: escaped
/// quotes, backslashes and control characters survive.
fn name_of_malformed_line(line: &str) -> Option<(String, Option<String>)> {
    let after_key = line.split(r#""name":"#).nth(1)?;
    let mut values = serde_json::Deserializer::from_str(after_key).into_iter::<String>();
    let name = values.next()?.ok()?;
    let rest = &after_key[values.byte_offset()..];
    let remote = rest
        .strip_prefix(r#","remote":"#)
        .and_then(|r| serde_json::Deserializer::from_str(r).into_iter::<Option<String>>().next())
        .and_then(Result::ok)
        .flatten();
    Some((name, remote))
}

fn is_real_remote(remote: &str) -> bool {
    !remote.is_empty() && remote != "git"
}

/// The pre-0.18 rule, for output without `remote`: a remote bookmark `name@remote` on
/// the local target owned by this bookmark means synced, any other means unsynced.
fn legacy_remote_status(name: &str, remote_bookmarks: &[String]) -> RemoteStatus {
    let non_git_remotes: Vec<&String> =
        remote_bookmarks.iter().filter(|rb| !rb.is_empty() && !rb.ends_with("@git")).collect();
    if non_git_remotes.is_empty() {
        return RemoteStatus::Local;
    }
    // Bookmark names may contain `@` (`feat@v2`), remote names in practice do not,
    // so the owner is everything before the last `@`.
    let synced = non_git_remotes.iter().any(|rb| rb.rsplit_once('@').is_some_and(|(owner, _)| owner == name));
    if synced { RemoteStatus::Synced } else { RemoteStatus::Unsynced }
}

/// Parse `jj bookmark list --template BOOKMARK_TEMPLATE` output.
///
/// Reports local bookmarks, in order. A bookmark is [`RemoteStatus::Unsynced`] when
/// a line for one of its (non-`git`) remote refs points at another commit or is
/// conflicted: by default jj lists a tracked remote bookmark separately only when it
/// points elsewhere. It is [`RemoteStatus::Synced`] when its own remote bookmark is
/// on its target (on a remote line, as `--all-remotes` prints, or among the target's
/// `remoteRefs`), and [`RemoteStatus::Local`] otherwise. Conflicted and locally
/// deleted bookmarks, which jj cannot render, are reported by name in `skipped`.
///
/// Output of the template before vcs-runner 0.18 (no `remote` key) is still read,
/// with its older rule: every line is a local bookmark unless its target has no
/// local bookmarks, and status comes from the remote bookmarks on the target.
pub fn parse_bookmark_output(output: &str) -> BookmarkParseResult {
    let mut raws = Vec::new();
    let mut skipped = Vec::new();
    let mut warned_names: HashSet<String> = HashSet::new();
    // Where each bookmark's remote refs point; `None` for a conflicted one, which jj
    // renders as `<Error: …>`.
    let mut remote_targets: HashMap<String, Vec<Option<String>>> = HashMap::new();

    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<RawBookmark>(line) {
            Ok(raw) => {
                if raw.remote_name().is_some_and(is_real_remote) {
                    remote_targets.entry(raw.name.clone()).or_default().push(Some(raw.commit_id.clone()));
                }
                raws.push(raw);
            }
            Err(_) => match name_of_malformed_line(line) {
                Some((name, Some(remote))) => {
                    if is_real_remote(&remote) {
                        remote_targets.entry(name).or_default().push(None);
                    }
                }
                found => {
                    let name = found.map_or_else(|| "<unknown>".to_string(), |(name, _)| name);
                    if warned_names.insert(name.clone()) {
                        skipped.push(name);
                    }
                }
            },
        }
    }

    let bookmarks = raws
        .iter()
        .filter_map(|raw| {
            let remote = match &raw.remote {
                None if raw.local_bookmarks.is_empty() => return None,
                None => legacy_remote_status(&raw.name, &raw.remote_bookmarks),
                Some(serde_json::Value::String(_)) => return None,
                Some(_) => remote_status(raw, remote_targets.get(&raw.name).map_or(&[][..], Vec::as_slice)),
            };
            Some(Bookmark {
                name: raw.name.clone(),
                commit_id: raw.commit_id.clone(),
                change_id: raw.change_id.clone(),
                remote,
            })
        })
        .collect();

    BookmarkParseResult { bookmarks, skipped }
}

fn remote_status(local: &RawBookmark, remote_targets: &[Option<String>]) -> RemoteStatus {
    if remote_targets.iter().any(|t| t.as_deref() != Some(local.commit_id.as_str())) {
        RemoteStatus::Unsynced
    } else if !remote_targets.is_empty() || local.remote_refs.iter().any(|(n, r)| *n == local.name && is_real_remote(r))
    {
        RemoteStatus::Synced
    } else {
        RemoteStatus::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERR: &str = "<Error: No Commit available>";

    fn local(name: &str, commit: &str, remote_refs: &str) -> String {
        format!(
            r#"{{"name":"{name}","remote":null,"commitId":"{commit}","changeId":"c{commit}","localBookmarks":["{name}"],"remoteRefs":[{remote_refs}]}}"#
        )
    }

    fn remote(name: &str, remote: &str, commit: &str) -> String {
        format!(
            r#"{{"name":"{name}","remote":"{remote}","commitId":"{commit}","changeId":"c{commit}","localBookmarks":[],"remoteRefs":[["{name}","{remote}"]]}}"#
        )
    }

    fn statuses(output: &str) -> Vec<(String, RemoteStatus)> {
        parse_bookmark_output(output).bookmarks.into_iter().map(|b| (b.name, b.remote)).collect()
    }

    #[test]
    fn empty_output() {
        let result = parse_bookmark_output("");
        assert!(result.bookmarks.is_empty());
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn a_bookmark_with_no_remote_is_local() {
        assert_eq!(
            statuses(&local("feature", "a1", r#"["feature","git"]"#)),
            vec![("feature".into(), RemoteStatus::Local)]
        );
    }

    #[test]
    fn its_own_remote_bookmark_on_the_target_is_synced() {
        let out = local("feature", "a1", r#"["feature","origin"],["feature","git"]"#);
        assert_eq!(statuses(&out), vec![("feature".into(), RemoteStatus::Synced)]);
    }

    #[test]
    fn another_bookmarks_remote_on_the_target_does_not_count() {
        // Captured shape: `local-only` shares its commit with `feature@origin`.
        let out = local("local-only", "a1", r#"["feature","origin"],["feat@v2","origin"]"#);
        assert_eq!(statuses(&out), vec![("local-only".into(), RemoteStatus::Local)]);
    }

    #[test]
    fn a_remote_line_means_unsynced_and_is_not_a_bookmark_itself() {
        // jj lists a tracked remote bookmark only when it points elsewhere.
        let out = [local("feature", "b2", ""), remote("feature", "origin", "a1")].join("\n");
        let result = parse_bookmark_output(&out);
        assert_eq!(result.bookmarks.len(), 1, "{:?}", result.bookmarks);
        assert_eq!(result.bookmarks[0].commit_id, "b2");
        assert_eq!(result.bookmarks[0].remote, RemoteStatus::Unsynced);
    }

    #[test]
    fn a_remote_line_at_the_local_target_is_synced() {
        // `--all-remotes` prints synced remote bookmarks too.
        let out = [local("feature", "b2", ""), remote("feature", "origin", "b2")].join("\n");
        assert_eq!(statuses(&out), vec![("feature".into(), RemoteStatus::Synced)]);
        let out =
            [local("feature", "b2", ""), remote("feature", "origin", "b2"), remote("feature", "up", "a1")].join("\n");
        assert_eq!(statuses(&out), vec![("feature".into(), RemoteStatus::Unsynced)], "one remote elsewhere");
    }

    #[test]
    fn a_git_remote_line_is_ignored() {
        let out = [local("feature", "b2", ""), remote("feature", "git", "a1")].join("\n");
        assert_eq!(statuses(&out), vec![("feature".into(), RemoteStatus::Local)]);
    }

    #[test]
    fn a_locally_deleted_bookmark_is_skipped_and_its_remote_line_ignored() {
        let out = [
            format!(r#"{{"name":"feat@v2","remote":null,"commitId":{ERR},"changeId":{ERR},"localBookmarks":[{ERR}],"remoteRefs":[{ERR}]}}"#),
            remote("feat@v2", "origin", "a1"),
        ]
        .join("\n");
        let result = parse_bookmark_output(&out);
        assert!(result.bookmarks.is_empty(), "{:?}", result.bookmarks);
        assert_eq!(result.skipped, vec!["feat@v2"]);
    }

    #[test]
    fn a_conflicted_remote_ref_is_not_skipped_but_makes_its_bookmark_unsynced() {
        let conflicted = format!(r#"{{"name":"feature","remote":"origin","commitId":{ERR},"changeId":{ERR}}}"#);
        let result = parse_bookmark_output(&[local("feature", "a1", r#"["feature","origin"]"#), conflicted].join("\n"));
        assert!(result.skipped.is_empty(), "{:?}", result.skipped);
        assert_eq!(result.bookmarks[0].remote, RemoteStatus::Unsynced);
        let on_git = format!(r#"{{"name":"feature","remote":"git","commitId":{ERR}}}"#);
        let result = parse_bookmark_output(&[local("feature", "a1", ""), on_git].join("\n"));
        assert!(result.skipped.is_empty());
        assert_eq!(result.bookmarks[0].remote, RemoteStatus::Local);
    }

    #[test]
    fn a_skipped_name_is_decoded_and_reported_once() {
        let line = format!(r#"{{"name":"a\"b\\c\u0005","remote":null,"commitId":{ERR}}}"#);
        let result = parse_bookmark_output(&[line.clone(), line].join("\n"));
        assert_eq!(result.skipped, vec!["a\"b\\c\u{5}"]);
    }

    #[test]
    fn unparseable_lines_are_unknown() {
        assert_eq!(parse_bookmark_output("not json at all").skipped, vec!["<unknown>"]);
        assert_eq!(name_of_malformed_line(r#"{"name":}"#), None);
        assert_eq!(name_of_malformed_line(""), None);
    }

    // --- output of the template before 0.18 (no `remote` key) ---

    fn legacy(name: &str, local_bookmarks: &str, remote_bookmarks: &str) -> String {
        format!(
            r#"{{"name":"{name}","commitId":"abc","changeId":"xyz","localBookmarks":[{local_bookmarks}],"remoteBookmarks":[{remote_bookmarks}]}}"#
        )
    }

    #[test]
    fn legacy_output_keeps_its_rule() {
        let cases = [
            (legacy("feature", r#""feature""#, ""), RemoteStatus::Local),
            (legacy("feature", r#""feature""#, r#""feature@git""#), RemoteStatus::Local),
            (legacy("feature", r#""feature""#, r#""feature@origin""#), RemoteStatus::Synced),
            (legacy("feature", r#""feature""#, r#""other@origin""#), RemoteStatus::Unsynced),
            (legacy("feat", r#""feat""#, r#""feat@v2@origin""#), RemoteStatus::Unsynced),
            (legacy("feat@v2", r#""feat@v2""#, r#""feat@v2@origin""#), RemoteStatus::Synced),
        ];
        for (line, want) in cases {
            assert_eq!(parse_bookmark_output(&line).bookmarks[0].remote, want, "{line}");
        }
        assert!(parse_bookmark_output(&legacy("gone", "", r#""gone@origin""#)).bookmarks.is_empty());
    }

    #[test]
    fn legacy_stale_line_is_skipped_by_name() {
        let line = format!(r#"{{"name":"feat/stale","commitId":{ERR},"changeId":{ERR}}}"#);
        assert_eq!(parse_bookmark_output(&line).skipped, vec!["feat/stale"]);
    }

    #[test]
    fn template_names_every_key_the_parser_reads() {
        for key in
            ["\"name\":", "\"remote\":", "\"commitId\":", "\"changeId\":", "\"localBookmarks\":", "\"remoteRefs\":"]
        {
            assert!(BOOKMARK_TEMPLATE.contains(key), "{key}");
        }
    }

    mod properties {
        use proptest::prelude::*;

        use super::*;

        /// How one bookmark stands, and so which lines jj prints for it.
        #[derive(Debug, Clone, Copy)]
        enum State {
            /// No remote bookmark of its own (another bookmark's, or one on `git`, may sit
            /// on its target).
            Local,
            /// Its remote bookmark is on its target: no line of its own, listed in
            /// `remoteRefs`.
            SyncedOnTarget,
            /// A remote line at the same commit, as `--all-remotes` prints.
            SyncedLine,
            /// A remote line at another commit.
            Behind,
            /// A remote line jj cannot render.
            RemoteConflicted,
            /// The local bookmark itself is conflicted: reported by name in `skipped`.
            Conflicted,
            /// Deleted locally, still on the remote: no bookmark at all.
            DeletedLocally,
        }

        fn state() -> impl Strategy<Value = State> {
            prop::sample::select(vec![
                State::Local,
                State::SyncedOnTarget,
                State::SyncedLine,
                State::Behind,
                State::RemoteConflicted,
                State::Conflicted,
                State::DeletedLocally,
            ])
        }

        fn json(s: &str) -> String {
            serde_json::to_string(s).expect("a string encodes")
        }

        fn line(name: &str, remote: Option<&str>, commit: &str, local: &[&str], refs: &[(&str, &str)]) -> String {
            let remote = remote.map_or_else(|| "null".to_string(), json);
            let local: Vec<String> = local.iter().map(|n| json(n)).collect();
            let refs: Vec<String> = refs.iter().map(|(n, r)| format!("[{},{}]", json(n), json(r))).collect();
            format!(
                r#"{{"name":{},"remote":{remote},"commitId":{},"changeId":{},"localBookmarks":[{}],"remoteRefs":[{}]}}"#,
                json(name),
                json(commit),
                json(&format!("c{commit}")),
                local.join(","),
                refs.join(",")
            )
        }

        proptest! {
            // Names are arbitrary text, `@` and quotes included; the remote may be any
            // real remote name. The expected status is built into the case, not derived
            // from the parser's rule.
            #[test]
            fn every_bookmark_reads_back_with_the_status_it_was_printed_with(
                bookmarks in prop::collection::btree_map(".{1,8}", (state(), any::<bool>()), 0..6),
                remote in prop::sample::select(vec!["origin", "upstream", "o@x"]),
            ) {
                let mut lines = Vec::new();
                let mut expected = Vec::new();
                let mut skipped = Vec::new();
                for (i, (name, (state, noise))) in bookmarks.iter().enumerate() {
                    let commit = format!("a{i}");
                    let mut refs = vec![(name.as_str(), "git")];
                    if *noise {
                        refs.push(("someone-else", remote));
                    }
                    let local = |refs: &[(&str, &str)]| line(name, None, &commit, &[name.as_str()], refs);
                    let status = match state {
                        State::Local => {
                            lines.push(local(&refs));
                            Some(RemoteStatus::Local)
                        }
                        State::SyncedOnTarget => {
                            refs.push((name.as_str(), remote));
                            lines.push(local(&refs));
                            Some(RemoteStatus::Synced)
                        }
                        State::SyncedLine => {
                            lines.push(local(&refs));
                            lines.push(line(name, Some(remote), &commit, &[], &[(name.as_str(), remote)]));
                            Some(RemoteStatus::Synced)
                        }
                        State::Behind => {
                            lines.push(local(&refs));
                            lines.push(line(name, Some(remote), &format!("b{i}"), &[], &[(name.as_str(), remote)]));
                            Some(RemoteStatus::Unsynced)
                        }
                        State::RemoteConflicted => {
                            lines.push(local(&refs));
                            lines.push(format!(
                                r#"{{"name":{},"remote":{},"commitId":<Error: No Commit available>}}"#,
                                json(name),
                                json(remote)
                            ));
                            Some(RemoteStatus::Unsynced)
                        }
                        State::Conflicted => {
                            lines.push(format!(
                                r#"{{"name":{},"remote":null,"commitId":<Error: No Commit available>}}"#,
                                json(name)
                            ));
                            skipped.push(name.clone());
                            None
                        }
                        State::DeletedLocally => {
                            lines.push(format!(
                                r#"{{"name":{},"remote":{},"commitId":<Error: No Commit available>}}"#,
                                json(name),
                                json(remote)
                            ));
                            None
                        }
                    };
                    if let Some(status) = status {
                        expected.push((name.clone(), commit.clone(), status));
                    }
                }

                let parsed = parse_bookmark_output(&lines.join("\n"));
                let got: Vec<(String, String, RemoteStatus)> =
                    parsed.bookmarks.into_iter().map(|b| (b.name, b.commit_id, b.remote)).collect();
                prop_assert_eq!(got, expected);
                prop_assert_eq!(parsed.skipped, skipped);
            }
        }
    }
}
