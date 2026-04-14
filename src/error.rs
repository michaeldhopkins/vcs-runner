use std::fmt;
use std::io;
use std::process::ExitStatus;

/// Error type for subprocess execution.
///
/// Distinguishes between:
/// - [`Spawn`](Self::Spawn): infrastructure failure (binary missing, fork failed, etc.)
/// - [`NonZeroExit`](Self::NonZeroExit): the command ran and reported failure via exit code
///
/// Callers that want to treat non-zero exits as legitimate in-band signals
/// (e.g., `git show <nonexistent-ref>` returning "not found") can pattern-match:
///
/// ```no_run
/// # use std::path::Path;
/// # use vcs_runner::{run_git, RunError};
/// let repo = Path::new("/repo");
/// let maybe_bytes = match run_git(repo, &["show", "maybe-missing-ref"]) {
///     Ok(output) => Some(output.stdout),
///     Err(RunError::NonZeroExit { .. }) => None,   // ref not found
///     Err(e) => return Err(e.into()),              // real failure bubbles up
/// };
/// # Ok::<(), anyhow::Error>(())
/// ```
#[derive(Debug)]
pub enum RunError {
    /// Failed to spawn the child process. The binary may be missing, the
    /// working directory may not exist, or the OS may have refused the fork.
    Spawn {
        program: String,
        source: io::Error,
    },
    /// The child process ran but exited non-zero. For captured commands,
    /// `stdout` and `stderr` contain what the process wrote before exiting.
    /// For inherited commands ([`crate::run_cmd_inherited`]), they are empty.
    NonZeroExit {
        program: String,
        args: Vec<String>,
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: String,
    },
}

impl RunError {
    /// The program name that failed (e.g., `"git"`, `"jj"`).
    pub fn program(&self) -> &str {
        match self {
            Self::Spawn { program, .. } => program,
            Self::NonZeroExit { program, .. } => program,
        }
    }

    /// The captured stderr, if any. Empty for spawn failures and inherited commands.
    pub fn stderr(&self) -> Option<&str> {
        match self {
            Self::NonZeroExit { stderr, .. } => Some(stderr),
            Self::Spawn { .. } => None,
        }
    }

    /// The exit status, if the process actually ran.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        match self {
            Self::NonZeroExit { status, .. } => Some(*status),
            Self::Spawn { .. } => None,
        }
    }

    /// Whether this error represents a non-zero exit (the command ran and reported failure).
    pub fn is_non_zero_exit(&self) -> bool {
        matches!(self, Self::NonZeroExit { .. })
    }

    /// Whether this error represents a spawn failure (couldn't start the process).
    pub fn is_spawn_failure(&self) -> bool {
        matches!(self, Self::Spawn { .. })
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { program, source } => {
                write!(f, "failed to spawn {program}: {source}")
            }
            Self::NonZeroExit {
                program,
                args,
                status,
                stderr,
                ..
            } => {
                let trimmed = stderr.trim();
                if trimmed.is_empty() {
                    write!(f, "{program} {} exited with {status}", args.join(" "))
                } else {
                    write!(
                        f,
                        "{program} {} exited with {status}: {trimmed}",
                        args.join(" ")
                    )
                }
            }
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            Self::NonZeroExit { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_error() -> RunError {
        RunError::Spawn {
            program: "git".into(),
            source: io::Error::new(io::ErrorKind::NotFound, "not found"),
        }
    }

    fn non_zero_exit(stderr: &str) -> RunError {
        // ExitStatus can only be constructed from platform-specific means;
        // use a command we know exits non-zero to get a real one.
        let status = std::process::Command::new("false")
            .status()
            .expect("false should be runnable");
        RunError::NonZeroExit {
            program: "git".into(),
            args: vec!["status".into()],
            status,
            stdout: Vec::new(),
            stderr: stderr.to_string(),
        }
    }

    #[test]
    fn program_returns_name() {
        assert_eq!(spawn_error().program(), "git");
        assert_eq!(non_zero_exit("").program(), "git");
    }

    #[test]
    fn stderr_only_for_non_zero_exit() {
        assert_eq!(spawn_error().stderr(), None);
        assert_eq!(non_zero_exit("boom").stderr(), Some("boom"));
    }

    #[test]
    fn exit_status_only_for_non_zero_exit() {
        assert!(spawn_error().exit_status().is_none());
        assert!(non_zero_exit("").exit_status().is_some());
    }

    #[test]
    fn is_non_zero_exit_predicate() {
        assert!(!spawn_error().is_non_zero_exit());
        assert!(non_zero_exit("").is_non_zero_exit());
    }

    #[test]
    fn is_spawn_failure_predicate() {
        assert!(spawn_error().is_spawn_failure());
        assert!(!non_zero_exit("").is_spawn_failure());
    }

    #[test]
    fn display_spawn_failure() {
        let msg = format!("{}", spawn_error());
        assert!(msg.contains("spawn"));
        assert!(msg.contains("git"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn display_non_zero_exit_with_stderr() {
        let msg = format!("{}", non_zero_exit("something broke"));
        assert!(msg.contains("git status"));
        assert!(msg.contains("something broke"));
    }

    #[test]
    fn display_non_zero_exit_empty_stderr() {
        let msg = format!("{}", non_zero_exit(""));
        assert!(msg.contains("git status"));
        assert!(msg.contains("exited"));
    }

    #[test]
    fn error_source_for_spawn() {
        use std::error::Error;
        let err = spawn_error();
        assert!(err.source().is_some());
    }

    #[test]
    fn error_source_none_for_exit() {
        use std::error::Error;
        let err = non_zero_exit("");
        assert!(err.source().is_none());
    }

    #[test]
    fn wraps_into_anyhow() {
        // RunError implementing std::error::Error means anyhow can wrap it
        let err = spawn_error();
        let _: anyhow::Error = err.into();
    }
}
