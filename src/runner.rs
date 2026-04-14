use std::borrow::Cow;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use backon::{BlockingRetryable, ExponentialBuilder};

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
///
/// Fails with [`RunError::NonZeroExit`] (carrying captured output) on non-zero exit,
/// or [`RunError::Spawn`] if the process couldn't start.
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

/// Run a `jj` command in a repo directory, returning captured output.
pub fn run_jj(repo_path: &Path, args: &[&str]) -> Result<RunOutput, RunError> {
    run_cmd_in(repo_path, "jj", args)
}

/// Run a `git` command in a repo directory, returning captured output.
pub fn run_git(repo_path: &Path, args: &[&str]) -> Result<RunOutput, RunError> {
    run_cmd_in(repo_path, "git", args)
}

/// Run a command in a directory with retry on transient errors.
///
/// Uses exponential backoff (100ms, 200ms, 400ms) with up to 3 retries.
/// The `is_transient` callback receives a [`RunError`] and returns whether to retry.
///
/// For convenience, [`run_jj_with_retry`] and [`run_git_with_retry`]
/// pre-fill the program name.
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
            .with_min_delay(std::time::Duration::from_millis(100))
            .with_max_times(3),
    )
    .when(is_transient)
    .call()
}

/// Run a `jj` command with retry on transient errors.
///
/// Shorthand for `run_with_retry(repo_path, "jj", args, is_transient)`.
pub fn run_jj_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool,
) -> Result<RunOutput, RunError> {
    run_with_retry(repo_path, "jj", args, is_transient)
}

/// Run a `git` command with retry on transient errors.
///
/// Shorthand for `run_with_retry(repo_path, "git", args, is_transient)`.
pub fn run_git_with_retry(
    repo_path: &Path,
    args: &[&str],
    is_transient: impl Fn(&RunError) -> bool,
) -> Result<RunOutput, RunError> {
    run_with_retry(repo_path, "git", args, is_transient)
}

/// Default transient error check for jj/git.
///
/// Retries on:
/// - `NonZeroExit` with `"stale"` in stderr — "The working copy is stale" (jj)
/// - `NonZeroExit` with `".lock"` in stderr — Lock file contention (git/jj)
///
/// Spawn failures are never treated as transient.
pub fn is_transient_error(err: &RunError) -> bool {
    match err {
        RunError::NonZeroExit { stderr, .. } => {
            stderr.contains("stale") || stderr.contains(".lock")
        }
        RunError::Spawn { .. } => false,
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

    #[test]
    fn transient_detects_stale() {
        assert!(is_transient_error(&fake_non_zero("The working copy is stale")));
    }

    #[test]
    fn transient_detects_stale_in_context() {
        assert!(is_transient_error(&fake_non_zero(
            "Error: The working copy is stale (not updated since op abc)"
        )));
    }

    #[test]
    fn transient_detects_lock() {
        assert!(is_transient_error(&fake_non_zero(
            "Unable to create .lock: File exists"
        )));
    }

    #[test]
    fn transient_detects_git_index_lock() {
        assert!(is_transient_error(&fake_non_zero(
            "fatal: Unable to create '/repo/.git/index.lock'"
        )));
    }

    #[test]
    fn transient_rejects_config_error() {
        assert!(!is_transient_error(&fake_non_zero(
            "Config error: no such revision"
        )));
    }

    #[test]
    fn transient_rejects_empty() {
        assert!(!is_transient_error(&fake_non_zero("")));
    }

    #[test]
    fn transient_never_retries_spawn_failure() {
        assert!(!is_transient_error(&fake_spawn()));
    }

    // --- run_cmd_inherited ---

    #[test]
    fn cmd_inherited_succeeds() {
        run_cmd_inherited("true", &[]).expect("true should succeed");
    }

    #[test]
    fn cmd_inherited_fails_on_nonzero() {
        let result = run_cmd_inherited("false", &[]);
        let err = result.expect_err("should fail");
        assert!(err.is_non_zero_exit());
        assert_eq!(err.program(), "false");
    }

    #[test]
    fn cmd_inherited_fails_on_missing_binary() {
        let result = run_cmd_inherited("nonexistent_binary_xyz_42", &[]);
        let err = result.expect_err("should fail");
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
    fn cmd_in_with_env_empty_env_same_as_cmd_in() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let output =
            run_cmd_in_with_env(tmp.path(), "pwd", &[], &[]).expect("should succeed");
        let pwd = output.stdout_lossy().trim().to_string();
        let expected = tmp.path().canonicalize().expect("canonicalize");
        let actual = std::path::Path::new(&pwd).canonicalize().expect("canonicalize pwd");
        assert_eq!(actual, expected);
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
}
