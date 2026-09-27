#![no_main]

//! ROUNDTRIP target: what `BOOKMARK_TEMPLATE` makes jj print, `parse_bookmark_output`
//! reads back exactly, remote status included.
//!
//! jj applies the template once per ref (captured from jj 0.33 through 0.45 in
//! `tests/fixtures/jj/`), so each fuzzed bookmark renders as jj renders it:
//!
//! - **A local line** (`"remote":null`), every string through `escape_json()`
//!   (serde_json's encoding, which jj uses). Its `remoteRefs` are the remote bookmarks
//!   on its target: its own synced remotes, plus fuzzed refs of other bookmarks.
//! - **A remote line** (`"remote":"<name>"`) for each of its tracked remotes that
//!   points elsewhere. A conflicted one has `<Error: No Commit available>` values
//!   and still counts as pointing elsewhere. A synced one gets a line (at the local
//!   target) only sometimes, as under `--all-remotes`.
//! - **Stale** (conflicted, or deleted locally): the local line has jj's
//!   `<Error: No Commit available>` in place of every value after `remote`, and its
//!   remote lines still print.
//!
//! Expected back: every non-stale bookmark, in order, with the status the target
//! *built* into it rather than one recomputed by the parser's rule: `Unsynced` if a
//! non-`git` remote points elsewhere, else `Synced` if a non-`git` remote of its own
//! sits on its target, else `Local`. Stale names come back in `skipped`, once each.
//!
//! Frame: names and remote names are arbitrary, `@` and quotes included, except that a
//! remote name is never empty (jj's never are) and a name is used by one bookmark only,
//! since jj cannot have two local bookmarks of the same name.

use std::collections::HashSet;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use vcs_runner::{Bookmark, RemoteStatus, parse_bookmark_output};

#[derive(Arbitrary, Debug)]
struct Remote {
    /// `None` is the special `git` remote, which never counts as a real remote.
    remote: Option<String>,
    synced: bool,
    /// A synced remote printed on its own line anyway, as `--all-remotes` does.
    listed: bool,
    /// A conflicted remote ref, which jj renders with `<Error: …>` values.
    conflicted: bool,
    /// Where it points when not synced.
    elsewhere: String,
}

#[derive(Arbitrary, Debug)]
struct Entry {
    name: String,
    commit_id: String,
    change_id: String,
    local_bookmarks: Vec<String>,
    remotes: Vec<Remote>,
    /// Other bookmarks' remote refs that happen to sit on this target.
    other_refs: Vec<(String, String)>,
    stale: bool,
}

const ERR: &str = "<Error: No Commit available>";

fn json(s: &str) -> String {
    serde_json::to_string(s).expect("a str always serializes")
}

fn remote_name(r: &Remote) -> String {
    match &r.remote {
        None => "git".to_string(),
        Some(name) if name.is_empty() => "origin".to_string(),
        Some(name) => name.clone(),
    }
}

fn is_real(remote: &str) -> bool {
    remote != "git"
}

fn refs_on_target(e: &Entry) -> Vec<(String, String)> {
    let own = e.remotes.iter().filter(|r| r.synced).map(|r| (e.name.clone(), remote_name(r)));
    own.chain(e.other_refs.iter().cloned()).collect()
}

fn expected_status(e: &Entry) -> RemoteStatus {
    if e.remotes.iter().any(|r| !r.synced && is_real(&remote_name(r))) {
        RemoteStatus::Unsynced
    } else if refs_on_target(e).iter().any(|(n, r)| *n == e.name && !r.is_empty() && is_real(r)) {
        RemoteStatus::Synced
    } else {
        RemoteStatus::Local
    }
}

fn render(e: &Entry) -> String {
    let mut out = if e.stale {
        format!(
            "{{\"name\":{},\"remote\":null,\"commitId\":{ERR},\"changeId\":{ERR},\"localBookmarks\":[{ERR}],\"remoteRefs\":[{ERR}]}}\n",
            json(&e.name)
        )
    } else {
        let local: Vec<String> = e.local_bookmarks.iter().map(|b| json(b)).collect();
        let refs: Vec<String> = refs_on_target(e).iter().map(|(n, r)| format!("[{},{}]", json(n), json(r))).collect();
        format!(
            "{{\"name\":{},\"remote\":null,\"commitId\":{},\"changeId\":{},\"localBookmarks\":[{}],\"remoteRefs\":[{}]}}\n",
            json(&e.name),
            json(&e.commit_id),
            json(&e.change_id),
            local.join(","),
            refs.join(","),
        )
    };
    for r in &e.remotes {
        let remote = remote_name(r);
        let target = if r.synced {
            // A synced remote gets its own line only under `--all-remotes`.
            if !r.listed {
                continue;
            }
            json(&e.commit_id)
        } else if r.conflicted {
            ERR.to_string()
        } else {
            json(&elsewhere(r, e))
        };
        out.push_str(&format!(
            "{{\"name\":{},\"remote\":{},\"commitId\":{target},\"changeId\":{target},\"localBookmarks\":[],\"remoteRefs\":[[{},{}]]}}\n",
            json(&e.name),
            json(&remote),
            json(&e.name),
            json(&remote),
        ));
    }
    out
}

/// Where an unsynced remote points: never the local target, or it would be synced.
fn elsewhere(r: &Remote, e: &Entry) -> String {
    if r.elsewhere == e.commit_id { format!("{}x", r.elsewhere) } else { r.elsewhere.clone() }
}

fuzz_target!(|raw: Vec<Entry>| {
    let mut names = HashSet::new();
    let entries: Vec<Entry> = raw.into_iter().filter(|e| names.insert(e.name.clone())).collect();
    let out: String = entries.iter().map(render).collect();
    let parsed = parse_bookmark_output(&out);

    let want: Vec<Bookmark> = entries
        .iter()
        .filter(|e| !e.stale)
        .map(|e| Bookmark {
            name: e.name.clone(),
            commit_id: e.commit_id.clone(),
            change_id: e.change_id.clone(),
            remote: expected_status(e),
        })
        .collect();
    assert_eq!(parsed.bookmarks, want, "output:\n{out}");

    let want_skipped: Vec<String> = entries.iter().filter(|e| e.stale).map(|e| e.name.clone()).collect();
    assert_eq!(parsed.skipped, want_skipped, "output:\n{out}");
});
