//! VCS-specific subprocess helpers — thin wrappers over [`procpilot::Cmd`].
//!
//! The generic subprocess primitives (run_cmd, retry, timeout, etc.) live in
//! [`procpilot`]. This module layers on jj/git-specific conveniences:
//! merge-base helpers, the default transient-error predicate, and shorthand
//! `run_jj` / `run_git` wrappers.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use procpilot::{Cmd, RetryPolicy, RunError, RunOutput};

use crate::jj_version::{divergent_change_id_query, installed_jj_version};
use crate::parse_op::{parse_id_lines, parse_operation_log};

/// Run a `jj` command in a repo directory, returning captured output.
pub fn run_jj(repo_path: &Path, args: &[&str]) -> Result<RunOutput, RunError> {
    Cmd::new("jj").in_dir(repo_path).args(args).run()
}

/// Run a `git` command in a repo directory, returning captured output.
pub fn run_git(repo_path: &Path, args: &[&str]) -> Result<RunOutput, RunError> {
    Cmd::new("git").in_dir(repo_path).args(args).run()
}

/// Run a `jj` command, returning lossy-decoded, trimmed stdout as a `String`.
///
/// Shorthand for `run_jj(repo_path, args)?.stdout_lossy().trim().to_string()`
/// — the most common pattern for callers that treat stdout as text.
pub fn run_jj_utf8(repo_path: &Path, args: &[&str]) -> Result<String, RunError> {
    let out = run_jj(repo_path, args)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `jj` command **working-copy-agnostically** — with `--ignore-working-copy`
/// prepended — returning lossy-decoded, trimmed stdout.
///
/// Use this for any operation that must not perturb the user's working copy: a
/// read that should not snapshot their in-progress edits (and so should not
/// create a spurious snapshot operation), or a fetch/push that never needs the
/// working copy. It also stays readable when a concurrent writer has left the
/// working copy stale — a plain [`run_jj_utf8`] errors "working copy is stale"
/// in exactly that case, which is the moment op-log/divergence reads matter most.
pub fn run_jj_utf8_ignore_wc(repo_path: &Path, args: &[&str]) -> Result<String, RunError> {
    let mut full = Vec::with_capacity(args.len() + 1);
    full.push("--ignore-working-copy");
    full.extend_from_slice(args);
    run_jj_utf8(repo_path, &full)
}

/// Run a `git` command, returning lossy-decoded, trimmed stdout as a `String`.
pub fn run_git_utf8(repo_path: &Path, args: &[&str]) -> Result<String, RunError> {
    let out = run_git(repo_path, args)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `jj` command with a timeout, returning trimmed stdout as a `String`.
pub fn run_jj_utf8_with_timeout(repo_path: &Path, args: &[&str], timeout: Duration) -> Result<String, RunError> {
    let out = run_jj_with_timeout(repo_path, args, timeout)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `git` command with a timeout, returning trimmed stdout as a `String`.
pub fn run_git_utf8_with_timeout(repo_path: &Path, args: &[&str], timeout: Duration) -> Result<String, RunError> {
    let out = run_git_with_timeout(repo_path, args, timeout)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `jj` command with retry, returning trimmed stdout as a `String`.
pub fn run_jj_utf8_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
) -> Result<String, RunError> {
    let out = run_jj_with_retry(repo_path, args, is_transient)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `git` command with retry, returning trimmed stdout as a `String`.
pub fn run_git_utf8_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
) -> Result<String, RunError> {
    let out = run_git_with_retry(repo_path, args, is_transient)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `jj` command with a timeout.
pub fn run_jj_with_timeout(repo_path: &Path, args: &[&str], timeout: Duration) -> Result<RunOutput, RunError> {
    Cmd::new("jj").in_dir(repo_path).args(args).timeout(timeout).run()
}

/// Run a `git` command with a timeout.
pub fn run_git_with_timeout(repo_path: &Path, args: &[&str], timeout: Duration) -> Result<RunOutput, RunError> {
    Cmd::new("git").in_dir(repo_path).args(args).timeout(timeout).run()
}

/// Run a `jj` command with retry on transient errors.
pub fn run_jj_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
) -> Result<RunOutput, RunError> {
    Cmd::new("jj").in_dir(repo_path).args(args).retry(RetryPolicy::default().when(is_transient)).run()
}

/// Run a `git` command with retry on transient errors.
pub fn run_git_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
) -> Result<RunOutput, RunError> {
    Cmd::new("git").in_dir(repo_path).args(args).retry(RetryPolicy::default().when(is_transient)).run()
}

/// Run a `jj` command with caller-driven cancellation.
///
/// When `cancel` is set to `true` the wrapper kills the child (SIGTERM →
/// SIGKILL after the procpilot default grace) and returns
/// [`RunError::Cancelled`]. If `cancel` is already set before spawn,
/// returns `Cancelled` without starting the child.
pub fn run_jj_cancellable(repo_path: &Path, args: &[&str], cancel: Arc<AtomicBool>) -> Result<RunOutput, RunError> {
    Cmd::new("jj").in_dir(repo_path).args(args).cancel(cancel).run()
}

/// Run a `git` command with caller-driven cancellation. See
/// [`run_jj_cancellable`] for semantics.
pub fn run_git_cancellable(repo_path: &Path, args: &[&str], cancel: Arc<AtomicBool>) -> Result<RunOutput, RunError> {
    Cmd::new("git").in_dir(repo_path).args(args).cancel(cancel).run()
}

/// Run a `jj` command with cancellation, returning trimmed stdout as a `String`.
pub fn run_jj_utf8_cancellable(repo_path: &Path, args: &[&str], cancel: Arc<AtomicBool>) -> Result<String, RunError> {
    let out = run_jj_cancellable(repo_path, args, cancel)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `git` command with cancellation, returning trimmed stdout as a `String`.
pub fn run_git_utf8_cancellable(repo_path: &Path, args: &[&str], cancel: Arc<AtomicBool>) -> Result<String, RunError> {
    let out = run_git_cancellable(repo_path, args, cancel)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `jj` command with retry on transient errors plus caller-driven cancellation.
///
/// Setting `cancel` short-circuits any pending backoff sleep and kills the
/// in-flight child. The default retry predicate does not retry
/// [`RunError::Cancelled`], so a cancelled attempt ends the loop.
pub fn run_jj_with_retry_cancellable(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
    cancel: Arc<AtomicBool>,
) -> Result<RunOutput, RunError> {
    Cmd::new("jj").in_dir(repo_path).args(args).retry(RetryPolicy::default().when(is_transient)).cancel(cancel).run()
}

/// Run a `git` command with retry on transient errors plus caller-driven cancellation.
/// See [`run_jj_with_retry_cancellable`] for semantics.
pub fn run_git_with_retry_cancellable(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
    cancel: Arc<AtomicBool>,
) -> Result<RunOutput, RunError> {
    Cmd::new("git").in_dir(repo_path).args(args).retry(RetryPolicy::default().when(is_transient)).cancel(cancel).run()
}

/// Run a `jj` command with retry + cancellation, returning trimmed stdout as a `String`.
pub fn run_jj_utf8_with_retry_cancellable(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
    cancel: Arc<AtomicBool>,
) -> Result<String, RunError> {
    let out = run_jj_with_retry_cancellable(repo_path, args, is_transient, cancel)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `git` command with retry + cancellation, returning trimmed stdout as a `String`.
pub fn run_git_utf8_with_retry_cancellable(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
    cancel: Arc<AtomicBool>,
) -> Result<String, RunError> {
    let out = run_git_with_retry_cancellable(repo_path, args, is_transient, cancel)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Find the merge base of two revisions in a jj repository.
///
/// Uses the revset `latest(::(a) & ::(b))` — the most recent common ancestor
/// of the two revisions. Returns `Ok(None)` when the revisions have no common
/// ancestor.
pub fn jj_merge_base(repo_path: &Path, a: &str, b: &str) -> Result<Option<String>, RunError> {
    let revset = format!("latest(::({a}) & ::({b}))");
    let id = run_jj_utf8(repo_path, &["log", "-r", &revset, "--no-graph", "--limit", "1", "-T", "commit_id"])?;
    Ok(if id.is_empty() { None } else { Some(id) })
}

// --- jj operation log ---
//
// A second jj process mutating the same working copy (e.g. a background watcher
// racing a foreground command) forks the operation log; jj then silently
// "reconciles divergent operations" by merging the two heads, which can corrupt
// the working state. These helpers let a caller record a known-good operation
// before mutating, detect a reconcile, and roll back to it. `jj-lib` exposes
// these natively but churns pre-1.0; these shell out to a version-agnostic
// `jj`, matching the rest of this crate.

/// The id of the current (head) operation in the operation log.
///
/// Record this before a batch of mutations to get a point to [`jj_op_restore`]
/// back to if a concurrent jj process forces jj to reconcile divergent
/// operation logs mid-way.
pub fn jj_current_operation_id(repo_path: &Path) -> Result<String, RunError> {
    // `id` renders the full operation id, which `op restore` accepts directly.
    // Working-copy-agnostic: reading the op head must not snapshot the working
    // copy (that would create the very op we're trying to record).
    run_jj_utf8_ignore_wc(repo_path, &["op", "log", "-n1", "--no-graph", "-T", "id"])
}

/// The recent operation-log entries, newest first, up to `limit` (`0` = all).
///
/// Used to spot a `"reconcile divergent operations"` entry (the signature of a
/// concurrent writer) and to walk back to a clean operation for recovery.
pub fn jj_operation_log(repo_path: &Path, limit: usize) -> Result<Vec<crate::JjOperation>, RunError> {
    const TEMPLATE: &str = r#"id ++ "\t" ++ description.first_line() ++ "\n""#;
    let limit_str = limit.to_string();
    let mut args: Vec<&str> = vec!["op", "log", "--no-graph", "-T", TEMPLATE];
    if limit > 0 {
        // Insert `-n <limit>` right after the `log` subcommand.
        args.splice(2..2, ["-n", limit_str.as_str()]);
    }
    // Working-copy-agnostic: reading the op log must not snapshot the working copy.
    let out = run_jj_utf8_ignore_wc(repo_path, &args)?;
    Ok(parse_operation_log(&out))
}

/// Change ids that are divergent (one change id on multiple visible commits) —
/// the persistent tell left by a concurrent op-log reconcile. Deduplicated: a
/// change divergent across N commits is reported once. Empty means clean.
pub fn jj_divergent_change_ids(repo_path: &Path) -> Result<Vec<String>, RunError> {
    // Working-copy-agnostic: a concurrent writer can leave the working copy stale,
    // and this signal — which exists to detect exactly that — must stay readable.
    let query = divergent_change_id_query(installed_jj_version());
    let out = run_jj_utf8_ignore_wc(repo_path, &[&["log", "--no-graph"][..], &query].concat())?;
    let mut ids = parse_id_lines(&out);
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Whether divergent changes exist as of a specific past operation. Lets a
/// caller walk back to the most recent operation whose state predates a
/// divergence, to [`jj_op_restore`] to.
pub fn jj_is_divergent_at_operation(repo_path: &Path, op_id: &str) -> Result<bool, RunError> {
    // `--at-operation` is a global flag and must precede the subcommand.
    // Working-copy-agnostic: reading a past operation's state must not snapshot.
    let query = divergent_change_id_query(installed_jj_version());
    let args = [&["--at-operation", op_id, "log", "--no-graph"][..], &query].concat();
    let out = run_jj_utf8_ignore_wc(repo_path, &args)?;
    Ok(!parse_id_lines(&out).is_empty())
}

/// The commit ids `revset` resolves to **as of a specific operation** — `<ws>@`
/// at op X, `main` at op X, or any revset. The building block for reconstructing
/// a ref's history from the operation log without parsing `op log --op-diff`
/// prose (jj exposes no structured op-diff, so text-scraping is the alternative,
/// and it is exactly the "op log is a safety net, not a tool" anti-pattern).
///
/// The revset is wrapped in `present(…)`, so an operation older than the thing
/// it names — before a workspace or bookmark existed — resolves to an empty vec
/// rather than erroring, letting a caller distinguish "absent here" from a real
/// failure. Working-copy-agnostic: inspecting a past operation must not snapshot.
pub fn jj_revset_at_operation(repo_path: &Path, revset: &str, op_id: &str) -> Result<Vec<String>, RunError> {
    // `--at-operation` is a global flag and must precede the subcommand.
    let wrapped = format!("present({revset})");
    let out = run_jj_utf8_ignore_wc(
        repo_path,
        &["--at-operation", op_id, "log", "-r", &wrapped, "--no-graph", "-T", r#"commit_id ++ "\n""#],
    )?;
    Ok(parse_id_lines(&out))
}

/// The distinct commit ids `revset` resolved to across the operation log, newest
/// first — jj's analog of `git reflog <ref>`, for any revset. The first-class
/// replacement for scraping `op log --op-diff` to answer "what did this ever
/// point at" (working-copy recovery, pre-rebase-head recovery, drift detection).
///
/// Walks operations newest-first (via [`jj_operation_log`]) and evaluates
/// [`jj_revset_at_operation`] at each, collecting distinct commit ids in the
/// order first seen. The walk **stops at the first operation where `revset`
/// resolves to nothing** — i.e. before the named ref existed — bounding it to
/// the ref's own lifetime rather than the entire op log; `limit` (`0` = no cap)
/// caps how many operations are examined as an extra guard on large logs.
///
/// That stop-at-absent bound suits *ref-like* revsets that come into being and
/// persist (`<ws>@`, a bookmark). For a revset that is *transiently* empty —
/// `divergent()`, `conflicts()` — it would end the walk immediately; evaluate
/// [`jj_revset_at_operation`] over an explicit op list from [`jj_operation_log`]
/// instead. Working-copy-agnostic. Cost is one `jj` invocation per operation
/// walked: jj offers no batched or structured op-diff, so this is inherent.
pub fn jj_revset_history(repo_path: &Path, revset: &str, limit: usize) -> Result<Vec<String>, RunError> {
    let ops = jj_operation_log(repo_path, limit)?;
    let mut seen = std::collections::HashSet::new();
    let mut history = Vec::new();
    for op in &ops {
        let at = jj_revset_at_operation(repo_path, revset, &op.id)?;
        if at.is_empty() {
            break; // older than the ref — nothing more of its lifetime to find
        }
        for cid in at {
            if seen.insert(cid.clone()) {
                history.push(cid);
            }
        }
    }
    Ok(history)
}

/// Roll the repo back to `op_id` (`jj op restore`). The recovery primitive:
/// pick a known-good operation instead of keeping jj's mangled auto-merge.
///
/// In a colocated repo this stays git-consistent — jj re-exports refs to git as
/// part of the restore operation.
pub fn jj_op_restore(repo_path: &Path, op_id: &str) -> Result<(), RunError> {
    run_jj(repo_path, &["op", "restore", op_id])?;
    Ok(())
}

/// Find the merge base of two revisions in a git repository.
///
/// Returns `Ok(None)` when git reports no common ancestor (exit code 1 with
/// empty output), `Ok(Some(sha))` when found, `Err(_)` for actual failures.
pub fn git_merge_base(repo_path: &Path, a: &str, b: &str) -> Result<Option<String>, RunError> {
    match run_git_utf8(repo_path, &["merge-base", a, b]) {
        Ok(id) => Ok(if id.is_empty() { None } else { Some(id) }),
        Err(RunError::NonZeroExit { status, .. }) if status.code() == Some(1) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Default transient-error check for jj/git.
///
/// Retries on `NonZeroExit` whose stderr contains `"stale"` or `".lock"`
/// (jj working-copy staleness, git/jj lock-file contention). Spawn failures
/// and timeouts are never treated as transient.
pub fn is_transient_error(err: &RunError) -> bool {
    procpilot::default_transient(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jj_installed() -> bool {
        procpilot::binary_available("jj")
    }

    fn git_installed() -> bool {
        procpilot::binary_available("git")
    }

    #[test]
    fn is_transient_matches_stale_and_lock() {
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(256)
        };
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(1)
        };
        let err = RunError::NonZeroExit {
            command: Cmd::new("jj").display(),
            status,
            stdout: vec![],
            stderr: "The working copy is stale".into(),
            attempts: 1,
        };
        assert!(is_transient_error(&err));
    }

    #[test]
    fn run_jj_fails_gracefully_when_not_installed() {
        if jj_installed() {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = run_jj(tmp.path(), &["status"]).expect_err("jj not installed");
        assert!(err.is_spawn_failure());
    }

    #[test]
    fn run_jj_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err =
            run_jj_cancellable(tmp.path(), &["status"], cancel).expect_err("preset cancel flag must return error");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_git_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err =
            run_git_cancellable(tmp.path(), &["status"], cancel).expect_err("preset cancel flag must return error");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_jj_with_retry_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_jj_with_retry_cancellable(tmp.path(), &["status"], is_transient_error, cancel)
            .expect_err("preset cancel flag must return error before retry");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_jj_utf8_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err =
            run_jj_utf8_cancellable(tmp.path(), &["status"], cancel).expect_err("preset cancel flag must return error");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_git_utf8_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_git_utf8_cancellable(tmp.path(), &["status"], cancel)
            .expect_err("preset cancel flag must return error");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_git_with_retry_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_git_with_retry_cancellable(tmp.path(), &["status"], is_transient_error, cancel)
            .expect_err("preset cancel flag must return error before retry");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_jj_utf8_with_retry_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_jj_utf8_with_retry_cancellable(tmp.path(), &["status"], is_transient_error, cancel)
            .expect_err("preset cancel flag must return error before retry");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_git_utf8_with_retry_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_git_utf8_with_retry_cancellable(tmp.path(), &["status"], is_transient_error, cancel)
            .expect_err("preset cancel flag must return error before retry");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn is_transient_does_not_retry_cancelled() {
        // CHANGELOG advertises this behavior — pin it so a future procpilot
        // bump can't silently flip the default-predicate semantics.
        let err = RunError::Cancelled {
            command: Cmd::new("jj").display(),
            stdout: vec![],
            stderr: String::new(),
            attempts: 1,
        };
        assert!(!is_transient_error(&err));
    }

    /// Run git in `dir` with a fixed identity and no signing, panicking on
    /// failure; returns trimmed stdout.
    fn git_ok(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("spawn git");
        assert!(out.status.success(), "git {args:?} failed: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A git repo with two commits on `main` and an orphan root on `lone`.
    fn merge_base_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let d = tmp.path();
        git_ok(d, &["init", "--quiet", "-b", "main"]);
        git_ok(d, &["commit", "--quiet", "--allow-empty", "-m", "one"]);
        git_ok(d, &["commit", "--quiet", "--allow-empty", "-m", "two"]);
        git_ok(d, &["checkout", "--quiet", "--orphan", "lone"]);
        git_ok(d, &["commit", "--quiet", "--allow-empty", "-m", "orphan"]);
        tmp
    }

    #[test]
    fn git_merge_base_finds_common_ancestor() {
        if !git_installed() {
            return;
        }
        let tmp = merge_base_repo();
        let first = git_ok(tmp.path(), &["rev-parse", "main~1"]);
        assert_eq!(git_merge_base(tmp.path(), "main", "main~1").unwrap(), Some(first));
    }

    // git exits 1 with no output when the histories share no commit.
    #[test]
    fn git_merge_base_returns_none_for_unrelated() {
        if !git_installed() {
            return;
        }
        let tmp = merge_base_repo();
        assert_eq!(git_merge_base(tmp.path(), "main", "lone").unwrap(), None);
    }

    // Any other failure (git exits 128 on an unknown revision) is an error,
    // not "no common ancestor".
    #[test]
    fn git_merge_base_errors_on_unknown_revision() {
        if !git_installed() {
            return;
        }
        let tmp = merge_base_repo();
        let err = git_merge_base(tmp.path(), "main", "no-such-rev").unwrap_err();
        match err {
            RunError::NonZeroExit { status, .. } => assert_ne!(status.code(), Some(1)),
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    // --- every wrapper returns the command's real output ---
    //
    // Each wrapper is a one-line forward to procpilot, so the way one goes wrong
    // is by dropping or replacing what the command printed. These run a real
    // command whose output is known and compare it exactly.

    const GENEROUS: Duration = Duration::from_secs(60);

    fn not_cancelled() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    fn trimmed(out: RunOutput) -> String {
        out.stdout_lossy().trim().to_string()
    }

    /// A retry predicate that, on its first call, runs `fix` (so the next
    /// attempt succeeds) and asks for a retry, then refuses any later retry so a
    /// second failure surfaces. Returns the predicate and its call count.
    fn fix_then_retry(
        fix: impl Fn() + Send + Sync + 'static,
    ) -> (impl Fn(&RunError) -> bool + Send + Sync + 'static, Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let predicate = move |_: &RunError| {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                fix();
                true
            } else {
                false
            }
        };
        (predicate, calls)
    }

    fn call_count(calls: &std::sync::atomic::AtomicUsize) -> usize {
        calls.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// For a git retry wrapper: `refs/heads/<branch>` does not exist yet, so the
    /// first `rev-parse --verify` fails; the predicate then creates it at `main`.
    fn git_branch_appears_on_retry(
        dir: &Path,
        branch: &str,
    ) -> (impl Fn(&RunError) -> bool + Send + Sync + 'static, Arc<std::sync::atomic::AtomicUsize>) {
        let dir = dir.to_path_buf();
        let branch = branch.to_string();
        fix_then_retry(move || {
            git_ok(&dir, &["branch", &branch, "main"]);
        })
    }

    #[test]
    fn git_wrappers_return_the_commands_output() {
        if !git_installed() {
            return;
        }
        let tmp = merge_base_repo();
        let d = tmp.path();
        let main = git_ok(d, &["rev-parse", "main"]);
        assert_eq!(main.len(), 40, "a full commit id: {main:?}");
        let args = ["rev-parse", "main"];

        assert_eq!(trimmed(run_git(d, &args).unwrap()), main);
        assert_eq!(run_git_utf8(d, &args).unwrap(), main);
        assert_eq!(trimmed(run_git_with_timeout(d, &args, GENEROUS).unwrap()), main);
        assert_eq!(run_git_utf8_with_timeout(d, &args, GENEROUS).unwrap(), main);
        assert_eq!(trimmed(run_git_cancellable(d, &args, not_cancelled()).unwrap()), main);
        assert_eq!(run_git_utf8_cancellable(d, &args, not_cancelled()).unwrap(), main);
    }

    #[test]
    fn git_retry_wrappers_return_the_output_of_the_attempt_that_succeeded() {
        if !git_installed() {
            return;
        }
        let tmp = merge_base_repo();
        let d = tmp.path();
        let main = git_ok(d, &["rev-parse", "main"]);
        let verify = |branch: &str| ["rev-parse".to_string(), "--verify".into(), format!("refs/heads/{branch}")];

        let args = verify("r1");
        let (when, calls) = git_branch_appears_on_retry(d, "r1");
        let out = run_git_with_retry(d, &args.each_ref().map(String::as_str), when).unwrap();
        assert_eq!((trimmed(out), call_count(&calls)), (main.clone(), 1));

        let args = verify("r2");
        let (when, calls) = git_branch_appears_on_retry(d, "r2");
        let out = run_git_utf8_with_retry(d, &args.each_ref().map(String::as_str), when).unwrap();
        assert_eq!((out, call_count(&calls)), (main.clone(), 1));

        let args = verify("r3");
        let (when, calls) = git_branch_appears_on_retry(d, "r3");
        let out =
            run_git_with_retry_cancellable(d, &args.each_ref().map(String::as_str), when, not_cancelled()).unwrap();
        assert_eq!((trimmed(out), call_count(&calls)), (main.clone(), 1));

        let args = verify("r4");
        let (when, calls) = git_branch_appears_on_retry(d, "r4");
        let out = run_git_utf8_with_retry_cancellable(d, &args.each_ref().map(String::as_str), when, not_cancelled())
            .unwrap();
        assert_eq!((out, call_count(&calls)), (main, 1));
    }

    // A failure the predicate declines is returned, not retried.
    #[test]
    fn git_retry_wrapper_returns_the_error_the_predicate_declines() {
        if !git_installed() {
            return;
        }
        let tmp = merge_base_repo();
        let (when, calls) = fix_then_retry(|| {});
        let err = run_git_utf8_with_retry(tmp.path(), &["rev-parse", "--verify", "refs/heads/never"], when)
            .expect_err("the branch never appears");
        assert!(matches!(err, RunError::NonZeroExit { .. }), "got {err:?}");
        assert_eq!(call_count(&calls), 2, "one retry, then the second failure is declined");
    }

    /// The full commit id `rev` resolves to, read without snapshotting.
    fn jj_commit_id(repo: &TestRepo, rev: &str) -> String {
        let out = repo.jj(&["--ignore-working-copy", "log", "-r", rev, "--no-graph", "-T", "commit_id"]);
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn jj_wrappers_return_the_commands_output() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let d = repo.path();
        let parent = jj_commit_id(&repo, "@-");
        assert_eq!(parent.len(), 40, "a full commit id: {parent:?}");
        let args = ["log", "-r", "@-", "--no-graph", "-T", "commit_id"];

        assert_eq!(trimmed(run_jj(d, &args).unwrap()), parent);
        assert_eq!(run_jj_utf8(d, &args).unwrap(), parent);
        assert_eq!(run_jj_utf8_ignore_wc(d, &args).unwrap(), parent);
        assert_eq!(trimmed(run_jj_with_timeout(d, &args, GENEROUS).unwrap()), parent);
        assert_eq!(run_jj_utf8_with_timeout(d, &args, GENEROUS).unwrap(), parent);
        assert_eq!(trimmed(run_jj_cancellable(d, &args, not_cancelled()).unwrap()), parent);
        assert_eq!(run_jj_utf8_cancellable(d, &args, not_cancelled()).unwrap(), parent);
    }

    #[test]
    fn jj_retry_wrappers_return_the_output_of_the_attempt_that_succeeded() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let d = repo.path();
        let parent = jj_commit_id(&repo, "@-");

        // `<bookmark>` does not exist yet, so the first `jj log` fails; the
        // predicate then creates it at @-.
        let bookmark_appears_on_retry = |name: &str| {
            let dir = d.to_path_buf();
            let name = name.to_string();
            let fix = move || {
                let ok = std::process::Command::new("jj")
                    .args(["bookmark", "create", &name, "-r", "@-"])
                    .current_dir(&dir)
                    .output()
                    .expect("jj bookmark create")
                    .status
                    .success();
                assert!(ok, "jj bookmark create {name} failed");
            };
            fix_then_retry(fix)
        };
        let log = |name: &'static str| ["log", "-r", name, "--no-graph", "-T", "commit_id"];

        let (when, calls) = bookmark_appears_on_retry("r1");
        let out = run_jj_with_retry(d, &log("r1"), when).unwrap();
        assert_eq!((trimmed(out), call_count(&calls)), (parent.clone(), 1));

        let (when, calls) = bookmark_appears_on_retry("r2");
        let out = run_jj_utf8_with_retry(d, &log("r2"), when).unwrap();
        assert_eq!((out, call_count(&calls)), (parent.clone(), 1));

        let (when, calls) = bookmark_appears_on_retry("r3");
        let out = run_jj_with_retry_cancellable(d, &log("r3"), when, not_cancelled()).unwrap();
        assert_eq!((trimmed(out), call_count(&calls)), (parent.clone(), 1));

        let (when, calls) = bookmark_appears_on_retry("r4");
        let out = run_jj_utf8_with_retry_cancellable(d, &log("r4"), when, not_cancelled()).unwrap();
        assert_eq!((out, call_count(&calls)), (parent, 1));
    }

    // --- jj operation-log helpers ---

    /// A throwaway jj repo (optionally colocated) with helpers for driving jj
    /// and git and for forcing an operation-log divergence the way a concurrent
    /// writer would.
    struct TestRepo {
        dir: tempfile::TempDir,
    }
    impl TestRepo {
        fn new(colocate: bool) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let mut args = vec!["git", "init"];
            if colocate {
                args.push("--colocate");
            }
            let ok = std::process::Command::new("jj")
                .args(&args)
                .current_dir(dir.path())
                .output()
                .expect("jj git init")
                .status
                .success();
            assert!(ok, "jj git init failed");
            Self { dir }
        }
        fn path(&self) -> &Path {
            self.dir.path()
        }
        fn jj(&self, args: &[&str]) -> std::process::Output {
            std::process::Command::new("jj")
                .args(["--config=user.name=T", "--config=user.email=t@e.com"])
                .args(args)
                .current_dir(self.path())
                .output()
                .expect("jj command")
        }
        fn git(&self, args: &[&str]) -> std::process::Output {
            std::process::Command::new("git").args(args).current_dir(self.path()).output().expect("git command")
        }
        fn git_out(&self, args: &[&str]) -> String {
            String::from_utf8_lossy(&self.git(args).stdout).trim().to_string()
        }
        /// Two commits, A then B (@).
        fn seed_two_commits(&self) {
            std::fs::write(self.path().join("f.txt"), "a\n").unwrap();
            self.jj(&["describe", "-m", "A"]);
            self.jj(&["new", "-m", "B"]);
        }
        /// Fork the op log at an older op and reconcile — leaves a divergence.
        fn force_divergence(&self) {
            let out = self.jj(&["op", "log", "--no-graph", "-T", "id ++ \"\\n\""]);
            let ops = String::from_utf8_lossy(&out.stdout);
            let earlier = ops.lines().nth(2).expect("older op").to_string();
            self.jj(&["--at-operation", &earlier, "describe", "-m", "FORKED"]);
            self.jj(&["status"]); // triggers the auto-reconcile
        }
    }

    #[test]
    fn jj_merge_base_finds_common_ancestor() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let parent = String::from_utf8(repo.jj(&["log", "-r", "@-", "--no-graph", "-T", "commit_id"]).stdout).unwrap();
        assert_eq!(parent.len(), 40, "a full commit id: {parent:?}");
        assert_eq!(jj_merge_base(repo.path(), "@", "@-").unwrap(), Some(parent));
    }

    // Every jj commit descends from root(), so histories that share nothing else
    // still meet there; only an empty revset gives None.
    #[test]
    fn jj_merge_base_of_unrelated_is_root_and_of_nothing_is_none() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let b = String::from_utf8(repo.jj(&["log", "-r", "@", "--no-graph", "-T", "commit_id"]).stdout).unwrap();
        repo.jj(&["new", "root()", "-m", "C"]);
        assert_eq!(jj_merge_base(repo.path(), "@", &b).unwrap(), Some("0".repeat(40)));
        assert_eq!(jj_merge_base(repo.path(), "@", "none()").unwrap(), None);
    }

    #[test]
    fn jj_merge_base_errors_on_unknown_revision() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        let err = jj_merge_base(repo.path(), "@", "no-such-rev").unwrap_err();
        assert!(matches!(err, RunError::NonZeroExit { .. }), "got {err:?}");
    }

    #[test]
    fn operation_log_detection_and_op_restore_recovery() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let path = repo.path();

        let good = jj_current_operation_id(path).expect("current op id");
        assert!(!good.is_empty());
        assert!(jj_divergent_change_ids(path).unwrap().is_empty(), "clean to start");

        repo.force_divergence();

        // Detection: divergence is present, and a reconcile op shows since good.
        assert!(!jj_divergent_change_ids(path).unwrap().is_empty(), "divergence detected");
        let ops = jj_operation_log(path, 0).unwrap();
        assert!(
            ops.iter().any(|o| o.description.contains("reconcile divergent operations")),
            "reconcile op should appear; got {ops:?}"
        );

        // Recovery clears it.
        jj_op_restore(path, &good).unwrap();
        assert!(jj_divergent_change_ids(path).unwrap().is_empty(), "restore should clear divergence");
    }

    #[test]
    fn operation_log_respects_limit() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let all = jj_operation_log(repo.path(), 0).unwrap();
        let two = jj_operation_log(repo.path(), 2).unwrap();
        assert!(all.len() > 2, "seeded repo should have several ops");
        assert_eq!(two.len(), 2, "limit should cap the returned ops");
        assert_eq!(two[0], all[0], "same newest-first ordering");
    }

    #[test]
    fn is_divergent_at_operation_distinguishes_clean_from_divergent() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let good = jj_current_operation_id(repo.path()).unwrap();
        repo.force_divergence();

        let head = &jj_operation_log(repo.path(), 1).unwrap()[0].id;
        assert!(jj_is_divergent_at_operation(repo.path(), head).unwrap(), "head op is divergent");
        assert!(!jj_is_divergent_at_operation(repo.path(), &good).unwrap(), "good op is clean");
    }

    #[test]
    fn divergent_change_ids_dedups_to_distinct_changes() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        repo.force_divergence();
        // One forked change spans two commits but is one change id.
        assert_eq!(jj_divergent_change_ids(repo.path()).unwrap().len(), 1);
    }

    #[test]
    fn op_log_reads_do_not_snapshot_the_working_copy() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let path = repo.path();
        // An uncommitted edit sitting in the working copy.
        std::fs::write(path.join("f.txt"), "a\nwip\n").unwrap();
        let wc_commit = |repo: &TestRepo| {
            String::from_utf8_lossy(
                &repo.jj(&["--ignore-working-copy", "log", "-r", "@", "--no-graph", "-T", "commit_id"]).stdout,
            )
            .trim()
            .to_string()
        };
        let before = wc_commit(&repo);

        // These reads historically snapshotted `@` (folding the edit in and
        // creating a spurious op); they must now leave the working copy untouched.
        let _ = jj_current_operation_id(path).unwrap();
        let _ = jj_operation_log(path, 5).unwrap();
        let _ = jj_divergent_change_ids(path).unwrap();

        assert_eq!(before, wc_commit(&repo), "op-log/divergence reads must not snapshot the working copy");
    }

    #[test]
    fn revset_at_operation_wraps_absence_and_stays_wc_agnostic() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let path = repo.path();

        // Before any `hist` bookmark exists, `present(hist)` resolves empty — not
        // an error — so a caller can tell "absent at this op" from a real failure.
        let before = jj_current_operation_id(path).unwrap();
        assert!(
            jj_revset_at_operation(path, "hist", &before).unwrap().is_empty(),
            "a ref absent at an operation resolves to empty, not an error"
        );

        // And the read must not snapshot an uncommitted edit into `@`.
        std::fs::write(path.join("f.txt"), "a\nwip\n").unwrap();
        let wc = || {
            String::from_utf8_lossy(
                &repo.jj(&["--ignore-working-copy", "log", "-r", "@", "--no-graph", "-T", "commit_id"]).stdout,
            )
            .trim()
            .to_string()
        };
        let wc_before = wc();
        let _ = jj_revset_at_operation(path, "@", &before).unwrap();
        assert_eq!(wc_before, wc(), "reading a revset at an operation must not snapshot the working copy");
    }

    #[test]
    fn revset_history_reflogs_a_bookmarks_moves_and_bounds_at_creation() {
        if !jj_installed() {
            return;
        }
        let repo = TestRepo::new(false);
        repo.seed_two_commits();
        let path = repo.path();
        let commit = |rev: &str| {
            String::from_utf8_lossy(
                &repo
                    .jj(&["--ignore-working-copy", "log", "-r", rev, "--no-graph", "-T", "commit_id", "--limit", "1"])
                    .stdout,
            )
            .trim()
            .to_string()
        };
        let a = commit("@-"); // commit A
        let b = commit("@"); // working-copy commit B

        // Point a bookmark at A, then move it to B: two distinct values in its life.
        repo.jj(&["bookmark", "create", "hist", "-r", &a]);
        repo.jj(&["bookmark", "set", "hist", "-r", &b]);

        // Newest-first, distinct, and bounded at the bookmark's creation — the walk
        // stops before `hist` existed rather than scanning the whole op log.
        assert_eq!(
            jj_revset_history(path, "hist", 0).unwrap(),
            vec![b.clone(), a.clone()],
            "reflog-order history of the moving bookmark"
        );
    }

    // --- colocation safety ---
    //
    // The risk is that op-log recovery leaves a colocated repo's git side (a
    // branch ref, HEAD) pointing somewhere jj no longer agrees with. It does
    // not: jj re-exports refs to git as part of the restore operation.

    fn assert_colocated_consistent(repo: &TestRepo, bookmark: &str) {
        let jj_commit =
            String::from_utf8_lossy(&repo.jj(&["log", "-r", bookmark, "--no-graph", "-T", "commit_id"]).stdout)
                .trim()
                .to_string();
        assert_eq!(
            jj_commit,
            repo.git_out(&["rev-parse", bookmark]),
            "git ref and jj bookmark for '{bookmark}' must agree"
        );
    }
    fn assert_git_healthy(repo: &TestRepo) {
        // jj before 0.38 never wrote git's empty tree object ("The empty tree is now
        // always written", jj 0.38 changelog), so fsck on any repo with an empty commit
        // reports it missing, op restore or not. That one line is jj's, not a defect.
        const EMPTY_TREE_MISSING: &str = "missing tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904";
        let fsck = repo.git(&["fsck", "--no-dangling"]);
        let report = format!("{}{}", String::from_utf8_lossy(&fsck.stdout), String::from_utf8_lossy(&fsck.stderr));
        let only_empty_tree = !report.trim().is_empty() && report.lines().all(|l| l.trim() == EMPTY_TREE_MISSING);
        assert!(fsck.status.success() || only_empty_tree, "git fsck: {report}");
        assert!(repo.git(&["status"]).status.success(), "git status must work");
    }

    #[test]
    fn colocation_consistent_for_a_plain_bookmark() {
        if !jj_installed() || !git_installed() {
            return;
        }
        let repo = TestRepo::new(true);
        assert!(repo.path().join(".git").exists() && repo.path().join(".jj").exists());
        std::fs::write(repo.path().join("f.txt"), "a\n").unwrap();
        repo.jj(&["describe", "-m", "A"]);
        repo.jj(&["bookmark", "create", "feat", "-r", "@"]);
        repo.jj(&["new", "-m", "B"]);

        let good = jj_current_operation_id(repo.path()).unwrap();
        assert_colocated_consistent(&repo, "feat");
        repo.force_divergence();
        jj_op_restore(repo.path(), &good).unwrap();

        assert!(jj_divergent_change_ids(repo.path()).unwrap().is_empty());
        assert_colocated_consistent(&repo, "feat");
        assert_git_healthy(&repo);
    }

    #[test]
    fn colocation_consistent_when_bookmark_moved() {
        if !jj_installed() || !git_installed() {
            return;
        }
        let repo = TestRepo::new(true);
        std::fs::write(repo.path().join("f.txt"), "a\n").unwrap();
        repo.jj(&["describe", "-m", "A"]);
        repo.jj(&["bookmark", "create", "feat", "-r", "@"]); // feat @ A
        repo.jj(&["new", "-m", "B"]);
        repo.jj(&["new", "-m", "C"]); // A <- B <- C=@
        // MOVE feat from A to B (a non-working-copy commit).
        repo.jj(&["bookmark", "set", "feat", "-r", "@-"]);
        assert_colocated_consistent(&repo, "feat");

        let good = jj_current_operation_id(repo.path()).unwrap();
        repo.force_divergence();
        jj_op_restore(repo.path(), &good).unwrap();

        assert!(jj_divergent_change_ids(repo.path()).unwrap().is_empty());
        assert_colocated_consistent(&repo, "feat");
        assert_git_healthy(&repo);
    }

    #[test]
    fn colocation_consistent_for_a_stack() {
        if !jj_installed() || !git_installed() {
            return;
        }
        let repo = TestRepo::new(true);
        std::fs::write(repo.path().join("f.txt"), "a\n").unwrap();
        repo.jj(&["describe", "-m", "A"]);
        repo.jj(&["bookmark", "create", "bottom", "-r", "@"]);
        repo.jj(&["new", "-m", "B"]);
        repo.jj(&["bookmark", "create", "top", "-r", "@"]);
        repo.jj(&["new", "-m", "C"]);

        let good = jj_current_operation_id(repo.path()).unwrap();
        assert_colocated_consistent(&repo, "bottom");
        assert_colocated_consistent(&repo, "top");
        repo.force_divergence();
        jj_op_restore(repo.path(), &good).unwrap();

        assert!(jj_divergent_change_ids(repo.path()).unwrap().is_empty());
        assert_colocated_consistent(&repo, "bottom");
        assert_colocated_consistent(&repo, "top");
        assert_git_healthy(&repo);
    }
}
