//! Every jj output parser against real output from every jj version captured under
//! `tests/fixtures/jj/<version>/`, one test per version, so a regression names the
//! version it broke.
//!
//! The fixtures are captured, not written: `capture_fixtures` (ignored by default)
//! drives a given jj binary through a scratch repository with renames, bookmarks of
//! every kind, a conflicted bookmark and a divergent change, and saves what it prints.
//! To add a version:
//!
//! ```sh
//! VCS_RUNNER_CAPTURE_JJ=/path/to/jj cargo test --lib capture_fixtures -- --ignored
//! ```
//!
//! then add a `version_test!` line below. Change and commit ids differ per capture;
//! the assertions are about structure and names, never ids.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::jj_version::{divergent_change_id_query, parse_jj_version};
use crate::parse_op::{parse_id_lines, parse_operation_log};
use crate::types::{FileChangeKind, RemoteStatus, WorkingCopy};
use crate::{BOOKMARK_TEMPLATE, LOG_TEMPLATE, parse_bookmark_output, parse_diff_summary, parse_log_output, parse_remote_list};

const OP_LOG_TEMPLATE: &str = r#"id ++ "\t" ++ description.first_line() ++ "\n""#;

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jj")
}

// --- capture --------------------------------------------------------------------

struct Capture {
    jj: PathBuf,
    config: PathBuf,
    repo: PathBuf,
}

impl Capture {
    fn run(&self, args: &[&str]) -> Output {
        Command::new(&self.jj)
            .args(args)
            .current_dir(&self.repo)
            .env("JJ_CONFIG", &self.config)
            .output()
            .expect("run jj")
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(out.status.success(), "jj {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).expect("jj printed UTF-8")
    }

    /// The first spelling this jj accepts: flags and pattern syntax moved across versions.
    fn first_ok(&self, spellings: &[&[&str]]) {
        let mut errors = Vec::new();
        for args in spellings {
            let out = self.run(args);
            if out.status.success() {
                return;
            }
            errors.push(format!("{args:?}: {}", String::from_utf8_lossy(&out.stderr)));
        }
        panic!("no spelling worked:\n{}", errors.join("\n"));
    }

    fn op_id(&self) -> String {
        self.ok(&["op", "log", "-n1", "--no-graph", "-T", "id", "--ignore-working-copy"])
    }
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, body).expect("write");
}

fn rename(root: &Path, from: &str, to: &str) {
    let to = root.join(to);
    std::fs::create_dir_all(to.parent().expect("parent")).expect("mkdir");
    std::fs::rename(root.join(from), to).expect("rename");
}

#[test]
#[ignore = "captures fixtures from the jj named by VCS_RUNNER_CAPTURE_JJ"]
fn capture_fixtures() {
    let jj = PathBuf::from(std::env::var("VCS_RUNNER_CAPTURE_JJ").expect("set VCS_RUNNER_CAPTURE_JJ"));
    let raw_version = String::from_utf8(Command::new(&jj).arg("--version").output().expect("jj --version").stdout)
        .expect("utf-8");
    let v = parse_jj_version(&raw_version).expect("a jj version");
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = tmp.path().join("config.toml");
    std::fs::write(
        &config,
        "[user]\nname = \"Test User\"\nemail = \"test@example.com\"\n\
         [ui]\ncolor = \"never\"\npaginate = \"never\"\n\
         [debug]\ncommit-timestamp = \"2001-02-03T04:05:06+07:00\"\noperation-timestamp = \"2001-02-03T04:05:06+07:00\"\n\
         [operation]\nhostname = \"host\"\nusername = \"user\"\n",
    )
    .expect("write config");
    let mut c = Capture { jj, config, repo: tmp.path().to_path_buf() };
    c.ok(&["git", "init", "remote"]);
    c.ok(&["git", "init", "--colocate", "repo"]);
    let remote = tmp.path().join("remote");
    let remote_git = if remote.join(".git").exists() { remote.join(".git") } else { remote.join(".jj/repo/store/git") };
    let remote_git = remote_git.canonicalize().expect("remote exists");
    c.repo = tmp.path().join("repo");
    let root = c.repo.clone();

    let files = [
        ("src/a.rs", "fn a() {}\n// a\n// a\n"),
        ("d/e/x.rs", "x\nx\nx\n"),
        ("d/k.rs", "k\nk\nk\n"),
        ("d/e/p.rs", "p\np\np\n"),
        ("top.rs", "top\ntop\ntop\n"),
        ("sp ace.rs", "space\nspace\n"),
        ("trail ", "trailing\n"),
        ("é.rs", "accent\naccent\n"),
        ("keep.rs", "keep\n"),
    ];
    for (rel, body) in files {
        write(&root, rel, body);
    }
    c.ok(&["commit", "-m", "base\n\nsecond line"]);
    // A bookmark-free sibling of the renames commit, made divergent at the end.
    c.ok(&["describe", "-m", "side"]);
    let side = c.ok(&["log", "-r", "@", "--no-graph", "-T", "change_id"]);
    c.ok(&["new", "@-"]);
    c.ok(&["git", "remote", "add", "origin", remote_git.to_str().expect("utf-8 path")]);
    c.ok(&["bookmark", "create", "feature", "\"feat@v2\"", "local-only", "synced", "-r", "@-"]);
    for pattern in ["exact:feat@v2", "exact:\"feat@v2\""] {
        let with_new: &[&str] = &["git", "push", "--remote", "origin", "--bookmark", "feature", "--bookmark", "synced", "--bookmark", pattern, "--allow-new"];
        let without: &[&str] = &["git", "push", "--remote", "origin", "--bookmark", "feature", "--bookmark", "synced", "--bookmark", pattern];
        if c.run(with_new).status.success() || c.run(without).status.success() {
            break;
        }
    }

    for (from, to) in [
        ("src/a.rs", "src/b.rs"),
        ("d/e/x.rs", "g/e/x.rs"),
        ("d/k.rs", "k.rs"),
        ("d/e/p.rs", "d/e/deeper/p.rs"),
        ("top.rs", "moved.rs"),
        ("sp ace.rs", "sp ace 2.rs"),
        ("é.rs", "ü.rs"),
    ] {
        rename(&root, from, to);
    }
    write(&root, "trail ", "trailing\nchanged\n");
    write(&root, "keep.rs", "keep\nchanged\n");
    write(&root, "new file.rs", "new\n");
    let diff_summary = c.ok(&["diff", "--summary", "-r", "@"]);
    c.ok(&["commit", "-m", "renames"]);
    c.ok(&["bookmark", "set", "feature", "-r", "@-"]);

    // A conflicted bookmark: two concurrent operations move it from base to the two
    // siblings, renames and side. (Moving it to a commit and that commit's descendant
    // does not conflict: jj resolves it.)
    c.ok(&["bookmark", "create", "conflicted", "-r", "@--"]);
    let before = c.op_id();
    c.ok(&["bookmark", "set", "conflicted", "-r", "@-"]);
    c.ok(&["--at-op", before.trim(), "--ignore-working-copy", "bookmark", "set", "conflicted", "-r", side.trim()]);
    c.ok(&["status"]);

    // A tracked bookmark deleted locally: jj lists it with no local target.
    c.first_ok(&[&["bookmark", "delete", "exact:feat@v2"], &["bookmark", "delete", "exact:\"feat@v2\""]]);

    // A divergent change: two concurrent operations describe the same commit.
    let before = c.op_id();
    c.ok(&["describe", "-r", side.trim(), "-m", "side v2"]);
    c.ok(&["--at-op", before.trim(), "--ignore-working-copy", "describe", "-r", side.trim(), "-m", "side v3"]);
    c.ok(&["status"]);

    let query = divergent_change_id_query(Some(v));
    let divergent = c.ok(&[&["log", "--no-graph", "--ignore-working-copy"][..], &query].concat());
    let captures = [
        ("version.txt", raw_version.clone()),
        ("diff-summary.txt", diff_summary),
        ("bookmark-list.txt", c.ok(&["bookmark", "list", "-T", BOOKMARK_TEMPLATE, "--ignore-working-copy"])),
        ("log.txt", c.ok(&["log", "-r", "all()", "--no-graph", "-T", LOG_TEMPLATE, "--ignore-working-copy"])),
        ("op-log.txt", c.ok(&["op", "log", "--no-graph", "-n", "6", "-T", OP_LOG_TEMPLATE, "--ignore-working-copy"])),
        ("divergent.txt", divergent),
        // The remote's URL is a scratch path on the capturing machine; it is not kept.
        ("remote-list.txt", c.ok(&["git", "remote", "list", "--ignore-working-copy"]).replace(remote_git.to_str().expect("utf-8 path"), "/scratch/remote.git")),
    ];
    let dir = fixtures_root().join(format!("{}.{}.{}", v.major, v.minor, v.patch));
    std::fs::create_dir_all(&dir).expect("mkdir fixtures");
    for (name, body) in captures {
        std::fs::write(dir.join(name), body).expect("write fixture");
    }
}

// --- per-version assertions -----------------------------------------------------

fn fixture(version: &str, name: &str) -> String {
    let path = fixtures_root().join(version).join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn check_version(version: &str) {
    let parsed = parse_jj_version(&fixture(version, "version.txt")).expect("version.txt parses");
    assert_eq!(format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch), version);
    check_diff_summary(version);
    check_bookmarks(version);
    check_log(version);
    check_op_log(version);
    check_divergent(version);
    let remotes = parse_remote_list(&fixture(version, "remote-list.txt"));
    assert_eq!(remotes.len(), 1, "jj {version}: {remotes:?}");
    assert_eq!(remotes[0].name, "origin", "jj {version}");
    assert_eq!(remotes[0].url, "/scratch/remote.git", "jj {version}");
}

fn check_diff_summary(version: &str) {
    let mut changes: Vec<(FileChangeKind, String, Option<String>)> = parse_diff_summary(&fixture(version, "diff-summary.txt"))
        .into_iter()
        .map(|c| {
            let show = |p: &Path| p.to_string_lossy().into_owned();
            (c.kind, show(&c.path), c.from_path.as_deref().map(show))
        })
        .collect();
    changes.sort_by(|a, b| a.1.cmp(&b.1));
    let r = |from: &str, to: &str| (FileChangeKind::Renamed, to.to_string(), Some(from.to_string()));
    let m = |path: &str| (FileChangeKind::Modified, path.to_string(), None);
    let mut want = vec![
        r("src/a.rs", "src/b.rs"),
        r("d/e/x.rs", "g/e/x.rs"),
        r("d/k.rs", "k.rs"),
        r("d/e/p.rs", "d/e/deeper/p.rs"),
        r("top.rs", "moved.rs"),
        r("sp ace.rs", "sp ace 2.rs"),
        r("é.rs", "ü.rs"),
        m("trail "),
        m("keep.rs"),
        (FileChangeKind::Added, "new file.rs".to_string(), None),
    ];
    want.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(changes, want, "jj {version} diff --summary");
}

fn check_bookmarks(version: &str) {
    let parsed = parse_bookmark_output(&fixture(version, "bookmark-list.txt"));
    let mut got: Vec<(String, RemoteStatus)> = parsed.bookmarks.iter().map(|b| (b.name.clone(), b.remote)).collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        got,
        vec![("feature".to_string(), RemoteStatus::Unsynced), ("local-only".to_string(), RemoteStatus::Local), ("synced".to_string(), RemoteStatus::Synced)],
        "jj {version} bookmark list"
    );
    let mut skipped = parsed.skipped.clone();
    skipped.sort();
    assert_eq!(skipped, vec!["conflicted".to_string(), "feat@v2".to_string()], "jj {version} skipped bookmarks");
}

fn check_log(version: &str) {
    let parsed = parse_log_output(&fixture(version, "log.txt"));
    assert!(parsed.skipped.is_empty(), "jj {version}: {:?}", parsed.skipped);
    let mut summaries: Vec<&str> = parsed.entries.iter().map(|e| e.summary()).collect();
    summaries.sort_unstable();
    // Root, base, renames, the divergent side pair, and the working-copy commit.
    assert_eq!(summaries, vec!["", "", "base", "renames", "side v2", "side v3"], "jj {version} log");
    let base = parsed.entries.iter().find(|e| e.summary() == "base").expect("base");
    assert_eq!(base.description, "base\n\nsecond line\n", "jj {version}");
    assert_eq!(base.author_name, "Test User", "jj {version}");
    assert_eq!(base.author_email, "test@example.com", "jj {version}");
    let mut bookmarks = base.local_bookmarks.clone();
    bookmarks.sort();
    assert_eq!(bookmarks, vec!["local-only", "synced"], "jj {version}: base's local bookmarks");
    assert!(base.remote_bookmarks.iter().any(|b| b == "feature@origin"), "jj {version}: {:?}", base.remote_bookmarks);
    // Remote bookmarks come in jj's revset symbol form, so a name needing quotes has them.
    assert!(
        base.remote_bookmarks.iter().any(|b| b == "\"feat@v2\"@origin"),
        "jj {version}: {:?}",
        base.remote_bookmarks
    );
    let wc: Vec<_> = parsed.entries.iter().filter(|e| e.working_copy == WorkingCopy::Current).collect();
    assert_eq!(wc.len(), 1, "jj {version}: one working-copy commit");
    assert!(wc[0].content.is_empty(), "jj {version}: the working copy is empty");
    let root = parsed.entries.iter().find(|e| e.parents.is_empty()).expect("root commit");
    assert!(root.content.is_empty(), "jj {version}");
    let divergent: Vec<_> = parsed.entries.iter().filter(|e| e.summary().starts_with("side")).collect();
    assert_eq!(divergent[0].change_id, divergent[1].change_id, "jj {version}: a divergent pair shares a change id");
}

fn check_op_log(version: &str) {
    let ops = parse_operation_log(&fixture(version, "op-log.txt"));
    assert_eq!(ops.len(), 6, "jj {version}: {ops:?}");
    assert!(ops.iter().all(|o| o.id.len() >= 64 && o.id.chars().all(|c| c.is_ascii_hexdigit())), "jj {version}: {ops:?}");
    assert!(
        ops.iter().any(|o| o.description.contains("reconcile divergent operations")),
        "jj {version}: {ops:?}"
    );
}

fn check_divergent(version: &str) {
    let ids = parse_id_lines(&fixture(version, "divergent.txt"));
    assert_eq!(ids.len(), 2, "jj {version}: one divergent change on two commits: {ids:?}");
    assert_eq!(ids[0], ids[1], "jj {version}");
}

#[test]
fn every_captured_version_has_a_test() {
    let tested: Vec<&str> = TESTED.to_vec();
    let mut captured: Vec<String> = std::fs::read_dir(fixtures_root())
        .expect("tests/fixtures/jj")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    captured.sort();
    assert!(captured.len() >= 2, "fixtures from at least two jj versions: {captured:?}");
    assert_eq!(captured, tested, "add a version_test! line for each captured version, and nothing else");
}

macro_rules! version_tests {
    ($($name:ident => $version:literal),* $(,)?) => {
        const TESTED: &[&str] = &[$($version),*];
        $(
            #[test]
            fn $name() {
                check_version($version);
            }
        )*
    };
}

version_tests! {
    jj_0_33_0 => "0.33.0",
    jj_0_36_0 => "0.36.0",
    jj_0_37_0 => "0.37.0",
    jj_0_38_0 => "0.38.0",
    jj_0_40_0 => "0.40.0",
    jj_0_45_1 => "0.45.1",
}
