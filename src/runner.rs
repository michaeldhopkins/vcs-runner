use std::borrow::Cow;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use backon::{BlockingRetryable, ExponentialBuilder};
use wait_timeout::ChildExt;

use crate::error::RunError;

/// Captured output from a successful command.
///
/// Stdout is stored as raw bytes to support binary content (e.g., image
/// diffs via `jj file show` or `git show`). Use [`stdout_lossy()`](RunOutput::stdout_lossy)
/// for the common case of text output.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl RunOutput {
    /// Decode stdout as UTF-8, replacing invalid sequences with `�`.
    ///
    /// Returns a `Cow` — zero-copy when the bytes are valid UTF-8,
    /// which they almost always are for git/jj text output.
    pub fn stdout_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }
}

/// Run an arbitrary command with inherited stdout/stderr (visible to user).
///
/// Fails if the command exits non-zero. For captured output, use
/// [`run_cmd`] or the VCS-specific [`run_jj`] / [`run_git`].
///
/// Returns [`RunError`] on failure. Because stdout/stderr are inherited,
/// the `NonZeroExit` variant carries empty `stdout` and `stderr`.
pub fn run_cmd_inherited(program: &str, args: &[&str]) -> Result<(), RunError> {
    let status = Command::new(program).args(args).status().map_err(|source| {
        RunError::Spawn {
            program: program.to_string(),
            source,
        }
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(RunError::NonZeroExit {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            status,
            stdout: Vec::new(),
            stderr: String::new(),
        })
    }
}

/// Run an arbitrary command, capturing stdout and stderr.
pub fn run_cmd(program: &str, args: &[&str]) -> Result<RunOutput, RunError> {
    let output = Command::new(program).args(args).output().map_err(|source| {
        RunError::Spawn {
            program: program.to_string(),
            source,
        }
    })?;

    check_output(program, args, output)
}

/// Run an arbitrary command in a specific directory, capturing output.
///
/// The `dir` parameter is first to match `run_jj(dir, args)` / `run_git(dir, args)`.
pub fn run_cmd_in(dir: &Path, program: &str, args: &[&str]) -> Result<RunOutput, RunError> {
    run_cmd_in_with_env(dir, program, args, &[])
}

/// Run a command in a specific directory with extra environment variables.
///
/// Each `(key, value)` pair is added to the child process environment.
/// The parent's environment is inherited; these vars are added on top.
///
/// ```no_run
/// # use std::path::Path;
/// # use vcs_runner::run_cmd_in_with_env;
/// let repo = Path::new("/repo");
/// let output = run_cmd_in_with_env(
///     repo, "git", &["add", "-N", "--", "file.rs"],
///     &[("GIT_INDEX_FILE", "/tmp/index.tmp")],
/// )?;
/// # Ok::<(), vcs_runner::RunError>(())
/// ```
pub fn run_cmd_in_with_env(
    dir: &Path,
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<RunOutput, RunError> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(dir);
    for &(key, val) in env {
        cmd.env(key, val);
    }
    let output = cmd.output().map_err(|source| RunError::Spawn {
        program: program.to_string(),
        source,
    })?;

    check_output(program, args, output)
}

/// Run a command in a directory, killing it if it exceeds `timeout`.
///
/// Uses background threads to drain stdout and stderr so a chatty process
/// can't block on pipe buffer overflow. On timeout, the child is killed
/// and any output collected before the kill is included in the error.
///
/// Returns [`RunError::Timeout`] if the process was killed.
/// Returns [`RunError::NonZeroExit`] if it completed with a non-zero status.
/// Returns [`RunError::Spawn`] if the process couldn't start.
///
/// # Caveat: grandchildren
///
/// Only the direct child process receives the kill signal. Grandchildren
/// (spawned by the child) become orphans and continue running, and they
/// may hold the stdout/stderr pipes open, delaying this function's return
/// until they exit naturally. This is rare for direct invocations of
/// `git`/`jj` but can matter for shell wrappers — use `exec` in the shell
/// command (e.g., `sh -c "exec git fetch"`) to replace the shell with the
/// target process and avoid the grandchild case.
///
/// ```no_run
/// # use std::path::Path;
/// # use std::time::Duration;
/// # use vcs_runner::{run_cmd_in_with_timeout, RunError};
/// let repo = Path::new("/repo");
/// match run_cmd_in_with_timeout(repo, "git", &["fetch"], Duration::from_secs(30)) {
///     Ok(_) => println!("fetched"),
///     Err(RunError::Timeout { elapsed, .. }) => {
///         eprintln!("fetch hung, killed after {elapsed:?}");
///     }
///     Err(e) => return Err(e.into()),
/// }
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn run_cmd_in_with_timeout(
    dir: &Path,
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<RunOutput, RunError> {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| RunError::Spawn {
            program: program.to_string(),
            source,
        })?;

    // Drain stdio in background threads so the child can't block on pipe buffers.
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stdout_handle = thread::spawn(move || read_to_end(stdout));
    let stderr_handle = thread::spawn(move || read_to_end(stderr));

    let start = Instant::now();
    let wait_result = child.wait_timeout(timeout);

    // If the child is still alive (timeout or wait error), kill it BEFORE joining
    // the stdio threads — otherwise those threads block forever on read.
    let outcome = match wait_result {
        Ok(Some(status)) => Outcome::Exited(status),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Outcome::TimedOut(start.elapsed())
        }
        Err(source) => {
            let _ = child.kill();
            let _ = child.wait();
            Outcome::WaitFailed(source)
        }
    };

    // Safe to join now: the child is dead, pipes are closed, threads will return EOF.
    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();
    let stderr_str = String::from_utf8_lossy(&stderr_bytes).into_owned();

    match outcome {
        Outcome::Exited(status) => {
            if status.success() {
                Ok(RunOutput {
                    stdout: stdout_bytes,
                    stderr: stderr_str,
                })
            } else {
                Err(RunError::NonZeroExit {
                    program: program.to_string(),
                    args: args.iter().map(|s| s.to_string()).collect(),
                    status,
                    stdout: stdout_bytes,
                    stderr: stderr_str,
                })
            }
        }
        Outcome::TimedOut(elapsed) => Err(RunError::Timeout {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            elapsed,
            stdout: stdout_bytes,
            stderr: stderr_str,
        }),
        Outcome::WaitFailed(source) => Err(RunError::Spawn {
            program: program.to_string(),
            source,
        }),
    }
}

enum Outcome {
    Exited(std::process::ExitStatus),
    TimedOut(Duration),
    WaitFailed(std::io::Error),
}

/// Run a `jj` command in a repo directory, returning captured output.
pub fn run_jj(repo_path: &Path, args: &[&str]) -> Result<RunOutput, RunError> {
    run_cmd_in(repo_path, "jj", args)
}

/// Run a `git` command in a repo directory, returning captured output.
pub fn run_git(repo_path: &Path, args: &[&str]) -> Result<RunOutput, RunError> {
    run_cmd_in(repo_path, "git", args)
}

/// Run a `jj` command with a timeout.
///
/// Shorthand for `run_cmd_in_with_timeout(repo_path, "jj", args, timeout)`.
pub fn run_jj_with_timeout(
    repo_path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<RunOutput, RunError> {
    run_cmd_in_with_timeout(repo_path, "jj", args, timeout)
}

/// Run a `git` command with a timeout.
///
/// Shorthand for `run_cmd_in_with_timeout(repo_path, "git", args, timeout)`.
pub fn run_git_with_timeout(
    repo_path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<RunOutput, RunError> {
    run_cmd_in_with_timeout(repo_path, "git", args, timeout)
}

/// Run a command in a directory with retry on transient errors.
///
/// Uses exponential backoff (100ms, 200ms, 400ms) with up to 3 retries.
/// The `is_transient` callback receives a [`RunError`] and returns whether to retry.
pub fn run_with_retry(
    repo_path: &Path,
    program: &str,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool,
) -> Result<RunOutput, RunError> {
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    let op = || {
        let str_args: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
        run_cmd_in(repo_path, program, &str_args)
    };

    op.retry(
        ExponentialBuilder::default()
            .with_factor(2.0)
            .with_min_delay(Duration::from_millis(100))
            .with_max_times(3),
    )
    .when(is_transient)
    .call()
}

/// Run a `jj` command with retry on transient errors.
pub fn run_jj_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool,
) -> Result<RunOutput, RunError> {
    run_with_retry(repo_path, "jj", args, is_transient)
}

/// Run a `git` command with retry on transient errors.
pub fn run_git_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool,
) -> Result<RunOutput, RunError> {
    run_with_retry(repo_path, "git", args, is_transient)
}

/// Find the merge base of two revisions in a jj repository.
///
/// Uses the revset `latest(::(a) & ::(b))` — the most recent common ancestor
/// of the two revisions. Returns `Ok(None)` when the revisions have no common
/// ancestor (unrelated histories).
///
/// ```no_run
/// # use std::path::Path;
/// # use vcs_runner::jj_merge_base;
/// let repo = Path::new("/repo");
/// let base = jj_merge_base(repo, "trunk()", "@")?;
/// # Ok::<(), vcs_runner::RunError>(())
/// ```
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
///
/// ```no_run
/// # use std::path::Path;
/// # use vcs_runner::git_merge_base;
/// let repo = Path::new("/repo");
/// let base = git_merge_base(repo, "origin/main", "HEAD")?;
/// # Ok::<(), vcs_runner::RunError>(())
/// ```
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
        // git merge-base exits 1 when no common ancestor exists.
        Err(RunError::NonZeroExit { status, .. }) if status.code() == Some(1) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Default transient error check for jj/git.
///
/// Retries on:
/// - `NonZeroExit` with `"stale"` in stderr — "The working copy is stale" (jj)
/// - `NonZeroExit` with `".lock"` in stderr — Lock file contention (git/jj)
///
/// Spawn failures and timeouts are never treated as transient.
pub fn is_transient_error(err: &RunError) -> bool {
    match err {
        RunError::NonZeroExit { stderr, .. } => {
            stderr.contains("stale") || stderr.contains(".lock")
        }
        RunError::Spawn { .. } | RunError::Timeout { .. } => false,
    }
}

/// Check whether a binary is available on PATH.
pub fn binary_available(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Get a binary's version string, if available.
pub fn binary_version(name: &str) -> Option<String> {
    let output = Command::new(name).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn check_output(program: &str, args: &[&str], output: Output) -> Result<RunOutput, RunError> {
    if output.status.success() {
        Ok(RunOutput {
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    } else {
        Err(RunError::NonZeroExit {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            status: output.status,
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn read_to_end<R: Read>(mut reader: R) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_transient_error ---

    fn fake_non_zero(stderr: &str) -> RunError {
        let status = Command::new("false").status().expect("false");
        RunError::NonZeroExit {
            program: "jj".into(),
            args: vec!["status".into()],
            status,
            stdout: Vec::new(),
            stderr: stderr.to_string(),
        }
    }

    fn fake_spawn() -> RunError {
        RunError::Spawn {
            program: "jj".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        }
    }

    fn fake_timeout() -> RunError {
        RunError::Timeout {
            program: "git".into(),
            args: vec!["fetch".into()],
            elapsed: Duration::from_secs(30),
            stdout: Vec::new(),
            stderr: String::new(),
        }
    }

    #[test]
    fn transient_detects_stale() {
        assert!(is_transient_error(&fake_non_zero("The working copy is stale")));
    }

    #[test]
    fn transient_detects_lock() {
        assert!(is_transient_error(&fake_non_zero(
            "Unable to create .lock: File exists"
        )));
    }

    #[test]
    fn transient_rejects_config_error() {
        assert!(!is_transient_error(&fake_non_zero(
            "Config error: no such revision"
        )));
    }

    #[test]
    fn transient_never_retries_spawn_failure() {
        assert!(!is_transient_error(&fake_spawn()));
    }

    #[test]
    fn transient_never_retries_timeout() {
        assert!(!is_transient_error(&fake_timeout()));
    }

    // --- run_cmd_inherited ---

    #[test]
    fn cmd_inherited_succeeds() {
        run_cmd_inherited("true", &[]).expect("true should succeed");
    }

    #[test]
    fn cmd_inherited_fails_on_nonzero() {
        let err = run_cmd_inherited("false", &[]).expect_err("should fail");
        assert!(err.is_non_zero_exit());
        assert_eq!(err.program(), "false");
    }

    #[test]
    fn cmd_inherited_fails_on_missing_binary() {
        let err = run_cmd_inherited("nonexistent_binary_xyz_42", &[]).expect_err("should fail");
        assert!(err.is_spawn_failure());
    }

    // --- run_cmd ---

    #[test]
    fn cmd_captured_succeeds() {
        let output = run_cmd("echo", &["hello"]).expect("echo should succeed");
        assert_eq!(output.stdout_lossy().trim(), "hello");
    }

    #[test]
    fn cmd_captured_fails_on_nonzero() {
        let err = run_cmd("false", &[]).expect_err("should fail");
        assert!(err.is_non_zero_exit());
        assert!(err.exit_status().is_some());
    }

    #[test]
    fn cmd_captured_captures_stderr_on_failure() {
        let err = run_cmd("sh", &["-c", "echo err >&2; exit 1"]).expect_err("should fail");
        assert_eq!(err.stderr(), Some("err\n"));
    }

    #[test]
    fn cmd_captured_captures_stdout_on_failure() {
        let err = run_cmd("sh", &["-c", "echo output; exit 1"]).expect_err("should fail");
        match &err {
            RunError::NonZeroExit { stdout, .. } => {
                assert_eq!(String::from_utf8_lossy(stdout).trim(), "output");
            }
            _ => panic!("expected NonZeroExit"),
        }
    }

    #[test]
    fn cmd_fails_on_missing_binary() {
        let err = run_cmd("nonexistent_binary_xyz_42", &[]).expect_err("should fail");
        assert!(err.is_spawn_failure());
    }

    // --- run_cmd_in ---

    #[test]
    fn cmd_in_runs_in_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let output = run_cmd_in(tmp.path(), "pwd", &[]).expect("pwd should work");
        let pwd = output.stdout_lossy().trim().to_string();
        let expected = tmp.path().canonicalize().expect("canonicalize");
        let actual = std::path::Path::new(&pwd).canonicalize().expect("canonicalize pwd");
        assert_eq!(actual, expected);
    }

    #[test]
    fn cmd_in_fails_on_nonzero() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = run_cmd_in(tmp.path(), "false", &[]).expect_err("should fail");
        assert!(err.is_non_zero_exit());
    }

    #[test]
    fn cmd_in_fails_on_nonexistent_dir() {
        let err = run_cmd_in(
            std::path::Path::new("/nonexistent_dir_xyz_42"),
            "echo",
            &["hi"],
        )
        .expect_err("should fail");
        assert!(err.is_spawn_failure());
    }

    // --- run_cmd_in_with_env ---

    #[test]
    fn cmd_in_with_env_sets_variable() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let output = run_cmd_in_with_env(
            tmp.path(),
            "sh",
            &["-c", "echo $TEST_VAR_XYZ"],
            &[("TEST_VAR_XYZ", "hello_from_env")],
        )
        .expect("should succeed");
        assert_eq!(output.stdout_lossy().trim(), "hello_from_env");
    }

    #[test]
    fn cmd_in_with_env_multiple_vars() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let output = run_cmd_in_with_env(
            tmp.path(),
            "sh",
            &["-c", "echo ${A}_${B}"],
            &[("A", "foo"), ("B", "bar")],
        )
        .expect("should succeed");
        assert_eq!(output.stdout_lossy().trim(), "foo_bar");
    }

    #[test]
    fn cmd_in_with_env_overrides_existing_var() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let output = run_cmd_in_with_env(
            tmp.path(),
            "sh",
            &["-c", "echo $HOME"],
            &[("HOME", "/fake/home")],
        )
        .expect("should succeed");
        assert_eq!(output.stdout_lossy().trim(), "/fake/home");
    }

    #[test]
    fn cmd_in_with_env_fails_on_nonzero() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = run_cmd_in_with_env(
            tmp.path(),
            "sh",
            &["-c", "exit 1"],
            &[("IRRELEVANT", "value")],
        )
        .expect_err("should fail");
        assert!(err.is_non_zero_exit());
    }

    // --- run_cmd_in_with_timeout ---

    #[test]
    fn timeout_succeeds_for_fast_command() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let output =
            run_cmd_in_with_timeout(tmp.path(), "echo", &["hello"], Duration::from_secs(5))
                .expect("should succeed");
        assert_eq!(output.stdout_lossy().trim(), "hello");
    }

    #[test]
    fn timeout_fires_for_slow_command() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let wall_start = Instant::now();
        let err = run_cmd_in_with_timeout(
            tmp.path(),
            "sleep",
            &["10"],
            Duration::from_millis(200),
        )
        .expect_err("should time out");
        let wall_elapsed = wall_start.elapsed();

        assert!(err.is_timeout());
        // The sleep was for 10 seconds; we killed it. Total wall time must be far less.
        assert!(
            wall_elapsed < Duration::from_secs(5),
            "expected quick kill, took {wall_elapsed:?}"
        );
    }

    #[test]
    fn timeout_captures_partial_stderr_before_kill() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // `exec sleep 10` replaces the shell with sleep directly, so killing our
        // direct child (sleep) closes its stderr pipe. Without exec, the shell
        // would fork sleep as a grandchild that survives the kill and keeps the
        // pipe open until its natural completion.
        let err = run_cmd_in_with_timeout(
            tmp.path(),
            "sh",
            &["-c", "echo partial >&2; exec sleep 10"],
            Duration::from_millis(500),
        )
        .expect_err("should time out");
        assert!(err.is_timeout());
        let stderr = err.stderr().unwrap_or("");
        assert!(
            stderr.contains("partial"),
            "expected partial stderr, got: {stderr:?}"
        );
    }

    #[test]
    fn timeout_reports_non_zero_exit_when_process_completes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = run_cmd_in_with_timeout(
            tmp.path(),
            "false",
            &[],
            Duration::from_secs(5),
        )
        .expect_err("should fail");
        assert!(err.is_non_zero_exit());
    }

    #[test]
    fn timeout_fails_on_missing_binary() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = run_cmd_in_with_timeout(
            tmp.path(),
            "nonexistent_binary_xyz_42",
            &[],
            Duration::from_secs(5),
        )
        .expect_err("should fail");
        assert!(err.is_spawn_failure());
    }

    #[test]
    fn timeout_does_not_block_on_large_output() {
        // Without background thread draining, a command that writes more than
        // the pipe buffer (usually 64KB) would block waiting for us to read.
        // The timeout would fire but the process would be hung on write.
        let tmp = tempfile::tempdir().expect("tempdir");
        let output = run_cmd_in_with_timeout(
            tmp.path(),
            "sh",
            &["-c", "yes | head -c 200000"],
            Duration::from_secs(5),
        )
        .expect("should succeed");
        assert!(output.stdout.len() >= 200_000);
    }

    // --- RunOutput ---

    #[test]
    fn stdout_lossy_valid_utf8() {
        let output = RunOutput {
            stdout: b"hello world".to_vec(),
            stderr: String::new(),
        };
        assert_eq!(output.stdout_lossy(), "hello world");
    }

    #[test]
    fn stdout_lossy_invalid_utf8() {
        let output = RunOutput {
            stdout: vec![0xff, 0xfe, b'a', b'b'],
            stderr: String::new(),
        };
        let s = output.stdout_lossy();
        assert!(s.contains("ab"));
        assert!(s.contains('�'));
    }

    #[test]
    fn stdout_raw_bytes_preserved() {
        let bytes: Vec<u8> = (0..=255).collect();
        let output = RunOutput {
            stdout: bytes.clone(),
            stderr: String::new(),
        };
        assert_eq!(output.stdout, bytes);
    }

    #[test]
    fn run_output_debug_impl() {
        let output = RunOutput {
            stdout: b"hello".to_vec(),
            stderr: "warn".to_string(),
        };
        let debug = format!("{output:?}");
        assert!(debug.contains("warn"));
        assert!(debug.contains("stdout"));
    }

    // --- binary_available / binary_version ---

    #[test]
    fn binary_available_true_returns_true() {
        assert!(binary_available("echo"));
    }

    #[test]
    fn binary_available_missing_returns_false() {
        assert!(!binary_available("nonexistent_binary_xyz_42"));
    }

    #[test]
    fn binary_version_missing_returns_none() {
        assert!(binary_version("nonexistent_binary_xyz_42").is_none());
    }

    // --- run_jj / run_git (only if binary available) ---

    #[test]
    fn run_jj_version_succeeds() {
        if !binary_available("jj") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let output = run_jj(tmp.path(), &["--version"]).expect("jj --version should work");
        assert!(output.stdout_lossy().contains("jj"));
    }

    #[test]
    fn run_jj_fails_in_non_repo() {
        if !binary_available("jj") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = run_jj(tmp.path(), &["status"]).expect_err("should fail");
        assert!(err.is_non_zero_exit());
    }

    #[test]
    fn run_git_version_succeeds() {
        if !binary_available("git") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let output = run_git(tmp.path(), &["--version"]).expect("git --version should work");
        assert!(output.stdout_lossy().contains("git"));
    }

    #[test]
    fn run_git_fails_in_non_repo() {
        if !binary_available("git") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = run_git(tmp.path(), &["status"]).expect_err("should fail");
        assert!(err.is_non_zero_exit());
    }

    #[test]
    fn run_jj_with_timeout_succeeds() {
        if !binary_available("jj") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let output =
            run_jj_with_timeout(tmp.path(), &["--version"], Duration::from_secs(5))
                .expect("jj --version should work");
        assert!(output.stdout_lossy().contains("jj"));
    }

    #[test]
    fn run_git_with_timeout_succeeds() {
        if !binary_available("git") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let output =
            run_git_with_timeout(tmp.path(), &["--version"], Duration::from_secs(5))
                .expect("git --version should work");
        assert!(output.stdout_lossy().contains("git"));
    }

    // --- check_output ---

    #[test]
    fn check_output_preserves_stderr_on_success() {
        let output =
            run_cmd("sh", &["-c", "echo ok; echo warn >&2"]).expect("should succeed");
        assert_eq!(output.stdout_lossy().trim(), "ok");
        assert_eq!(output.stderr.trim(), "warn");
    }

    // --- retry ---

    #[test]
    fn retry_accepts_closure_over_run_error() {
        let captured = "special".to_string();
        let checker = |err: &RunError| err.stderr().is_some_and(|s| s.contains(captured.as_str()));

        assert!(!checker(&fake_non_zero("other")));
        assert!(checker(&fake_non_zero("this has special text")));
        assert!(!checker(&fake_spawn()));
    }

    // --- merge_base helpers ---

    #[test]
    fn git_merge_base_finds_common_ancestor() {
        if !binary_available("git") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path();

        // Init repo with one commit
        let _ = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo)
            .output();
        let _ = Command::new("git")
            .args(["config", "user.email", "t@t"])
            .current_dir(repo)
            .output();
        let _ = Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(repo)
            .output();
        std::fs::write(repo.join("a.txt"), "a").expect("write test file");
        let _ = Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output();
        let _ = Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(repo)
            .output();

        // HEAD is merge base with itself
        let base = git_merge_base(repo, "HEAD", "HEAD").expect("should succeed");
        assert!(base.is_some());
        assert_eq!(base.as_deref().map(str::len), Some(40));
    }

    #[test]
    fn git_merge_base_unrelated_histories_returns_none() {
        if !binary_available("git") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path();

        // Init with one commit on main, then create an orphan branch.
        // Use -b main explicitly since CI's git may default to master.
        let _ = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo)
            .output();
        let _ = Command::new("git")
            .args(["config", "user.email", "t@t"])
            .current_dir(repo)
            .output();
        let _ = Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(repo)
            .output();
        std::fs::write(repo.join("a.txt"), "a").expect("write test file");
        let _ = Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output();
        let _ = Command::new("git")
            .args(["commit", "-m", "main-1"])
            .current_dir(repo)
            .output();
        let _ = Command::new("git")
            .args(["checkout", "--orphan", "alt"])
            .current_dir(repo)
            .output();
        let _ = Command::new("git")
            .args(["rm", "-rf", "."])
            .current_dir(repo)
            .output();
        std::fs::write(repo.join("b.txt"), "b").expect("write test file");
        let _ = Command::new("git")
            .args(["add", "."])
            .current_dir(repo)
            .output();
        let _ = Command::new("git")
            .args(["commit", "-m", "alt-1"])
            .current_dir(repo)
            .output();

        // alt and main have no common ancestor
        let result = git_merge_base(repo, "alt", "main");
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn git_merge_base_invalid_ref_returns_err() {
        if !binary_available("git") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let _ = Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output();
        let result = git_merge_base(tmp.path(), "nonexistent-ref-xyz", "HEAD");
        // Invalid refs produce exit code 128, which is a real error (not None).
        assert!(result.is_err());
    }

    #[test]
    fn jj_merge_base_same_rev_returns_self() {
        if !binary_available("jj") {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path();
        let _ = Command::new("jj")
            .args(["git", "init"])
            .current_dir(repo)
            .output();

        // @ with @ should be @ itself
        let base = jj_merge_base(repo, "@", "@");
        // Empty fresh repo — @ exists but may or may not have a commit_id
        // depending on jj version. Just check the call succeeds.
        assert!(base.is_ok());
    }
}
