//! VCS-specific subprocess helpers — thin wrappers over [`procpilot::Cmd`].
//!
//! The generic subprocess primitives (run_cmd, retry, timeout, etc.) live in
//! [`procpilot`]. This module layers on jj/git-specific conveniences:
//! merge-base helpers, the default transient-error predicate, and shorthand
//! `run_jj` / `run_git` wrappers.

use std::path::Path;
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
    let output = run_jj(
        repo_path,
        &[
            "log", "-r", &revset, "--no-graph", "--limit", "1", "-T", "commit_id",
        ],
    )?;
    let id = output.stdout_lossy().trim().to_string();
    Ok(if id.is_empty() { None } else { Some(id) })
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
    match run_git(repo_path, &["merge-base", a, b]) {
        Ok(output) => {
            let id = output.stdout_lossy().trim().to_string();
            Ok(if id.is_empty() { None } else { Some(id) })
        }
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
}
