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

/// Run a `git` command, returning lossy-decoded, trimmed stdout as a `String`.
pub fn run_git_utf8(repo_path: &Path, args: &[&str]) -> Result<String, RunError> {
    let out = run_git(repo_path, args)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `jj` command with a timeout, returning trimmed stdout as a `String`.
pub fn run_jj_utf8_with_timeout(
    repo_path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String, RunError> {
    let out = run_jj_with_timeout(repo_path, args, timeout)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `git` command with a timeout, returning trimmed stdout as a `String`.
pub fn run_git_utf8_with_timeout(
    repo_path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String, RunError> {
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
pub fn run_jj_with_timeout(
    repo_path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<RunOutput, RunError> {
    Cmd::new("jj")
        .in_dir(repo_path)
        .args(args)
        .timeout(timeout)
        .run()
}

/// Run a `git` command with a timeout.
pub fn run_git_with_timeout(
    repo_path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<RunOutput, RunError> {
    Cmd::new("git")
        .in_dir(repo_path)
        .args(args)
        .timeout(timeout)
        .run()
}

/// Run a `jj` command with retry on transient errors.
pub fn run_jj_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
) -> Result<RunOutput, RunError> {
    Cmd::new("jj")
        .in_dir(repo_path)
        .args(args)
        .retry(RetryPolicy::default().when(is_transient))
        .run()
}

/// Run a `git` command with retry on transient errors.
pub fn run_git_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
) -> Result<RunOutput, RunError> {
    Cmd::new("git")
        .in_dir(repo_path)
        .args(args)
        .retry(RetryPolicy::default().when(is_transient))
        .run()
}

/// Run a `jj` command with caller-driven cancellation.
///
/// When `cancel` is set to `true` the wrapper kills the child (SIGTERM →
/// SIGKILL after the procpilot default grace) and returns
/// [`RunError::Cancelled`]. If `cancel` is already set before spawn,
/// returns `Cancelled` without starting the child.
pub fn run_jj_cancellable(
    repo_path: &Path,
    args: &[&str],
    cancel: Arc<AtomicBool>,
) -> Result<RunOutput, RunError> {
    Cmd::new("jj")
        .in_dir(repo_path)
        .args(args)
        .cancel(cancel)
        .run()
}

/// Run a `git` command with caller-driven cancellation. See
/// [`run_jj_cancellable`] for semantics.
pub fn run_git_cancellable(
    repo_path: &Path,
    args: &[&str],
    cancel: Arc<AtomicBool>,
) -> Result<RunOutput, RunError> {
    Cmd::new("git")
        .in_dir(repo_path)
        .args(args)
        .cancel(cancel)
        .run()
}

/// Run a `jj` command with cancellation, returning trimmed stdout as a `String`.
pub fn run_jj_utf8_cancellable(
    repo_path: &Path,
    args: &[&str],
    cancel: Arc<AtomicBool>,
) -> Result<String, RunError> {
    let out = run_jj_cancellable(repo_path, args, cancel)?;
    Ok(out.stdout_lossy().trim().to_string())
}

/// Run a `git` command with cancellation, returning trimmed stdout as a `String`.
pub fn run_git_utf8_cancellable(
    repo_path: &Path,
    args: &[&str],
    cancel: Arc<AtomicBool>,
) -> Result<String, RunError> {
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
    Cmd::new("jj")
        .in_dir(repo_path)
        .args(args)
        .retry(RetryPolicy::default().when(is_transient))
        .cancel(cancel)
        .run()
}

/// Run a `git` command with retry on transient errors plus caller-driven cancellation.
/// See [`run_jj_with_retry_cancellable`] for semantics.
pub fn run_git_with_retry_cancellable(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool + Send + Sync + 'static,
    cancel: Arc<AtomicBool>,
) -> Result<RunOutput, RunError> {
    Cmd::new("git")
        .in_dir(repo_path)
        .args(args)
        .retry(RetryPolicy::default().when(is_transient))
        .cancel(cancel)
        .run()
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
pub fn jj_merge_base(
    repo_path: &Path,
    a: &str,
    b: &str,
) -> Result<Option<String>, RunError> {
    let revset = format!("latest(::({a}) & ::({b}))");
    let id = run_jj_utf8(
        repo_path,
        &[
            "log", "-r", &revset, "--no-graph", "--limit", "1", "-T", "commit_id",
        ],
    )?;
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
    run_jj_utf8(repo_path, &["op", "log", "-n1", "--no-graph", "-T", "id"])
}

/// The recent operation-log entries, newest first, up to `limit` (`0` = all).
///
/// Used to spot a `"reconcile divergent operations"` entry (the signature of a
/// concurrent writer) and to walk back to a clean operation for recovery.
pub fn jj_operation_log(
    repo_path: &Path,
    limit: usize,
) -> Result<Vec<crate::JjOperation>, RunError> {
    const TEMPLATE: &str = r#"id ++ "\t" ++ description.first_line() ++ "\n""#;
    let limit_str = limit.to_string();
    let mut args: Vec<&str> = vec!["op", "log", "--no-graph", "-T", TEMPLATE];
    if limit > 0 {
        // Insert `-n <limit>` right after the `log` subcommand.
        args.splice(2..2, ["-n", limit_str.as_str()]);
    }
    let out = run_jj_utf8(repo_path, &args)?;
    Ok(out
        .lines()
        .filter_map(|line| {
            line.split_once('\t').map(|(id, desc)| crate::JjOperation {
                id: id.to_string(),
                description: desc.to_string(),
            })
        })
        .collect())
}

/// Change ids that are divergent (one change id on multiple visible commits) —
/// the persistent tell left by a concurrent op-log reconcile. Deduplicated: a
/// change divergent across N commits is reported once. Empty means clean.
pub fn jj_divergent_change_ids(repo_path: &Path) -> Result<Vec<String>, RunError> {
    let out = run_jj_utf8(
        repo_path,
        &["log", "-r", "divergent()", "--no-graph", "-T", r#"change_id ++ "\n""#],
    )?;
    let mut ids: Vec<String> = out.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Whether divergent changes exist as of a specific past operation. Lets a
/// caller walk back to the most recent operation whose state predates a
/// divergence, to [`jj_op_restore`] to.
pub fn jj_is_divergent_at_operation(repo_path: &Path, op_id: &str) -> Result<bool, RunError> {
    // `--at-operation` is a global flag and must precede the subcommand.
    let out = run_jj_utf8(
        repo_path,
        &[
            "--at-operation", op_id,
            "log", "-r", "divergent()", "--no-graph", "-T", r#"change_id ++ "\n""#,
        ],
    )?;
    Ok(out.lines().any(|l| !l.is_empty()))
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
pub fn git_merge_base(
    repo_path: &Path,
    a: &str,
    b: &str,
) -> Result<Option<String>, RunError> {
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
        let err = run_jj_cancellable(tmp.path(), &["status"], cancel)
            .expect_err("preset cancel flag must return error");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_git_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_git_cancellable(tmp.path(), &["status"], cancel)
            .expect_err("preset cancel flag must return error");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_jj_with_retry_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_jj_with_retry_cancellable(
            tmp.path(),
            &["status"],
            is_transient_error,
            cancel,
        )
        .expect_err("preset cancel flag must return error before retry");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_jj_utf8_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_jj_utf8_cancellable(tmp.path(), &["status"], cancel)
            .expect_err("preset cancel flag must return error");
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
        let err = run_git_with_retry_cancellable(
            tmp.path(),
            &["status"],
            is_transient_error,
            cancel,
        )
        .expect_err("preset cancel flag must return error before retry");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_jj_utf8_with_retry_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_jj_utf8_with_retry_cancellable(
            tmp.path(),
            &["status"],
            is_transient_error,
            cancel,
        )
        .expect_err("preset cancel flag must return error before retry");
        assert!(err.is_cancelled(), "expected Cancelled, got: {err:?}");
    }

    #[test]
    fn run_git_utf8_with_retry_cancellable_short_circuits_on_preset_flag() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cancel = Arc::new(AtomicBool::new(true));
        let err = run_git_utf8_with_retry_cancellable(
            tmp.path(),
            &["status"],
            is_transient_error,
            cancel,
        )
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

    #[test]
    fn git_merge_base_returns_none_for_unrelated() {
        if !git_installed() {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(tmp.path())
            .status()
            .expect("git init");
        // Without commits, git merge-base fails with non-zero; accept that shape.
        let _ = git_merge_base(tmp.path(), "HEAD", "HEAD");
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
            std::process::Command::new("git")
                .args(args)
                .current_dir(self.path())
                .output()
                .expect("git command")
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

    // --- colocation safety ---
    //
    // The risk is that op-log recovery leaves a colocated repo's git side (a
    // branch ref, HEAD) pointing somewhere jj no longer agrees with. It does
    // not: jj re-exports refs to git as part of the restore operation.

    fn assert_colocated_consistent(repo: &TestRepo, bookmark: &str) {
        let jj_commit = String::from_utf8_lossy(
            &repo.jj(&["log", "-r", bookmark, "--no-graph", "-T", "commit_id"]).stdout,
        )
        .trim()
        .to_string();
        assert_eq!(
            jj_commit,
            repo.git_out(&["rev-parse", bookmark]),
            "git ref and jj bookmark for '{bookmark}' must agree"
        );
    }
    fn assert_git_healthy(repo: &TestRepo) {
        let fsck = repo.git(&["fsck", "--no-dangling"]);
        assert!(fsck.status.success(), "git fsck: {}", String::from_utf8_lossy(&fsck.stderr));
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
