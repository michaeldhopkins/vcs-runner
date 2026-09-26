#![no_main]

//! ROUNDTRIP target: what `BOOKMARK_TEMPLATE` makes jj print, `parse_bookmark_output`
//! reads back exactly, remote status included.
//!
//! Each fuzzed bookmark is rendered one of two ways, as jj renders it:
//!
//! - **Well-formed**: every string through `escape_json()` (serde_json's encoding, which
//!   jj uses), each remote bookmark as `stringify(name ++ "@" ++ remote)`. Expected
//!   back, in order, unless its `localBookmarks` list is empty (a remote-only entry,
//!   which the parser drops by design).
//! - **Stale**: a bookmark pointing at a missing commit, where jj prints
//!   `<Error: No Commit available>` in place of every value after the name. Expected in
//!   `skipped`, by its exact name, once per distinct name.
//!
//! Remote status is not recomputed with the parser's own rule. The target *builds*
//! each remote bookmark from a choice (this bookmark's own name, or another name) and
//! a remote, so it knows the answer: `Local` with no remote but `git`, `Synced` when
//! one of them is this bookmark's own, `Unsynced` otherwise. Bookmark names are
//! arbitrary, `@` included (git and jj both allow `feat@v2`). Remote names are
//! sanitized to hold no `@` and not be empty: that is what makes `name@remote`
//! readable at all, and jj's remote names come from git's, which never contain `@` in
//! practice.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use vcs_runner::{Bookmark, RemoteStatus, parse_bookmark_output};

#[derive(Arbitrary, Debug)]
enum Owner {
    This,
    Other(String),
}

#[derive(Arbitrary, Debug)]
struct Remote {
    owner: Owner,
    /// `None` is the special `git` remote, which never counts as a real remote.
    remote: Option<String>,
}

#[derive(Arbitrary, Debug)]
struct Entry {
    name: String,
    commit_id: String,
    change_id: String,
    local_bookmarks: Vec<String>,
    remotes: Vec<Remote>,
    stale: bool,
}

fn json(s: &str) -> String {
    serde_json::to_string(s).expect("a str always serializes")
}

fn remote_name(r: &Remote) -> String {
    match &r.remote {
        None => "git".to_string(),
        Some(name) => {
            let clean = name.replace('@', "");
            if clean.is_empty() || clean == "git" { "origin".to_string() } else { clean }
        }
    }
}

fn owner_name<'a>(r: &'a Remote, this: &'a str) -> &'a str {
    match &r.owner {
        Owner::This => this,
        Owner::Other(other) => other,
    }
}

fn expected_status(e: &Entry) -> RemoteStatus {
    let real: Vec<&Remote> = e.remotes.iter().filter(|r| remote_name(r) != "git").collect();
    if real.is_empty() {
        RemoteStatus::Local
    } else if real.iter().any(|r| owner_name(r, &e.name) == e.name) {
        RemoteStatus::Synced
    } else {
        RemoteStatus::Unsynced
    }
}

fn render(e: &Entry) -> String {
    if e.stale {
        const ERR: &str = "<Error: No Commit available>";
        return format!(
            "{{\"name\":{},\"commitId\":{ERR},\"changeId\":{ERR},\"localBookmarks\":[{ERR}],\"remoteBookmarks\":[{ERR}]}}\n",
            json(&e.name)
        );
    }
    let local: Vec<String> = e.local_bookmarks.iter().map(|b| json(b)).collect();
    let remote: Vec<String> = e
        .remotes
        .iter()
        .map(|r| json(&format!("{}@{}", owner_name(r, &e.name), remote_name(r))))
        .collect();
    format!(
        "{{\"name\":{},\"commitId\":{},\"changeId\":{},\"localBookmarks\":[{}],\"remoteBookmarks\":[{}]}}\n",
        json(&e.name),
        json(&e.commit_id),
        json(&e.change_id),
        local.join(","),
        remote.join(","),
    )
}

fuzz_target!(|entries: Vec<Entry>| {
    let out: String = entries.iter().map(render).collect();
    let parsed = parse_bookmark_output(&out);

    let want: Vec<Bookmark> = entries
        .iter()
        .filter(|e| !e.stale && !e.local_bookmarks.is_empty())
        .map(|e| Bookmark {
            name: e.name.clone(),
            commit_id: e.commit_id.clone(),
            change_id: e.change_id.clone(),
            remote: expected_status(e),
        })
        .collect();
    assert_eq!(parsed.bookmarks, want);

    let mut want_skipped: Vec<String> = Vec::new();
    for e in entries.iter().filter(|e| e.stale) {
        if !want_skipped.contains(&e.name) {
            want_skipped.push(e.name.clone());
        }
    }
    assert_eq!(parsed.skipped, want_skipped);
});
