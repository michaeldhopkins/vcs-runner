#![no_main]

//! ROUNDTRIP target: what `LOG_TEMPLATE` makes jj print, `parse_log_output` reads back
//! exactly; and the same for the `jj op log` template `jj_operation_log` uses.
//!
//! Fuzzes the *fields* and renders the line the way jj does: each string through
//! `escape_json()` (serde_json's string encoding, which jj uses), the three flags as
//! the quoted strings `"true"` / `"false"` the template emits, one entry per line.
//! Every field must come back byte-for-byte, and in order. The one documented
//! transformation is that the parser drops empty strings from the id and bookmark
//! lists, so the expectation does too.
//!
//! The op log is plain text, not JSON, so its fields are sanitized to what jj can
//! print there rather than skipped: an id never holds a tab or line break, and
//! `description.first_line()` holds no `\n` and cannot end in `\r` (a trailing `\r`
//! is eaten by `str::lines` as half of a CRLF, and jj's operation descriptions are
//! its own text, never a user's CRLF).

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use vcs_runner::fuzz_api::parse_operation_log;
use vcs_runner::{ConflictState, ContentState, JjOperation, WorkingCopy, parse_log_output};

#[derive(Arbitrary, Debug)]
struct Entry {
    commit_id: String,
    change_id: String,
    author_name: String,
    author_email: String,
    description: String,
    parents: Vec<String>,
    local_bookmarks: Vec<String>,
    remote_bookmarks: Vec<String>,
    working_copy: bool,
    conflict: bool,
    empty: bool,
}

#[derive(Arbitrary, Debug)]
struct Op {
    id: String,
    description: String,
}

#[derive(Arbitrary, Debug)]
struct Input {
    entries: Vec<Entry>,
    ops: Vec<Op>,
}

fn json(s: &str) -> String {
    serde_json::to_string(s).expect("a str always serializes")
}

fn json_list(items: &[String]) -> String {
    items.iter().map(|s| json(s)).collect::<Vec<_>>().join(",")
}

fn flag(b: bool) -> &'static str {
    if b { "\"true\"" } else { "\"false\"" }
}

fn render(e: &Entry) -> String {
    format!(
        "{{\"commitId\":{},\"changeId\":{},\"authorName\":{},\"authorEmail\":{},\"description\":{},\"parents\":[{}],\"localBookmarks\":[{}],\"remoteBookmarks\":[{}],\"isWorkingCopy\":{},\"conflict\":{},\"empty\":{}}}\n",
        json(&e.commit_id),
        json(&e.change_id),
        json(&e.author_name),
        json(&e.author_email),
        json(&e.description),
        json_list(&e.parents),
        json_list(&e.local_bookmarks),
        json_list(&e.remote_bookmarks),
        flag(e.working_copy),
        flag(e.conflict),
        flag(e.empty),
    )
}

fn non_empty(items: &[String]) -> Vec<String> {
    items.iter().filter(|s| !s.is_empty()).cloned().collect()
}

fn check_log(entries: &[Entry]) {
    let out: String = entries.iter().map(render).collect();
    let parsed = parse_log_output(&out);
    assert!(parsed.skipped.is_empty(), "a well-formed line was skipped: {:?}", parsed.skipped);
    assert_eq!(parsed.entries.len(), entries.len());
    for (got, want) in parsed.entries.iter().zip(entries) {
        assert_eq!(got.commit_id, want.commit_id);
        assert_eq!(got.change_id, want.change_id);
        assert_eq!(got.author_name, want.author_name);
        assert_eq!(got.author_email, want.author_email);
        assert_eq!(got.description, want.description);
        assert_eq!(got.parents, non_empty(&want.parents));
        assert_eq!(got.local_bookmarks, non_empty(&want.local_bookmarks));
        assert_eq!(got.remote_bookmarks, non_empty(&want.remote_bookmarks));
        let wc = if want.working_copy { WorkingCopy::Current } else { WorkingCopy::Background };
        assert_eq!(got.working_copy, wc);
        let conflict = if want.conflict { ConflictState::Conflicted } else { ConflictState::Clean };
        assert_eq!(got.conflict, conflict);
        let content = if want.empty { ContentState::Empty } else { ContentState::HasContent };
        assert_eq!(got.content, content);
        assert_eq!(got.summary(), want.description.lines().next().unwrap_or(""));
    }
}

fn check_op_log(ops: &[Op]) {
    let expected: Vec<JjOperation> = ops
        .iter()
        .map(|op| JjOperation {
            id: op.id.replace(['\t', '\n', '\r'], ""),
            description: op.description.replace('\n', "").trim_end_matches('\r').to_string(),
        })
        .collect();
    let out: String = expected.iter().map(|op| format!("{}\t{}\n", op.id, op.description)).collect();
    assert_eq!(parse_operation_log(&out), expected);
}

fuzz_target!(|input: Input| {
    check_log(&input.entries);
    check_op_log(&input.ops);
});
