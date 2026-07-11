//! VCS-specific helpers built on [`procpilot`]. Adds jj/git shorthand
//! wrappers, repo detection, and output parsers.
//!
//! For generic subprocess execution (stdin, retry, timeout, custom envs),
//! use [`procpilot::Cmd`] directly — it's re-exported here for convenience.

mod detect;
mod error;
#[cfg(feature = "git-parse")]
mod parse_git;
#[cfg(feature = "jj-parse")]
mod parse_jj;
mod runner;
mod types;
mod worktree;

pub use detect::{VcsBackend, detect_vcs};
pub use error::RunError;
#[cfg(feature = "git-parse")]
pub use parse_git::parse_git_diff_name_status;
#[cfg(feature = "jj-parse")]
pub use parse_jj::{
    BOOKMARK_TEMPLATE, LOG_TEMPLATE, BookmarkParseResult, LogParseResult, parse_bookmark_output,
    parse_diff_summary, parse_log_output, parse_remote_list,
};
pub use runner::{
    git_merge_base, is_transient_error, jj_current_operation_id, jj_divergent_change_ids,
    jj_is_divergent_at_operation, jj_merge_base, jj_op_restore, jj_operation_log,
    jj_revset_at_operation, jj_revset_history, run_git,
    run_git_cancellable, run_git_utf8, run_git_utf8_cancellable, run_git_utf8_with_retry,
    run_git_utf8_with_retry_cancellable, run_git_utf8_with_timeout, run_git_with_retry,
    run_git_with_retry_cancellable, run_git_with_timeout, run_jj, run_jj_cancellable, run_jj_utf8,
    run_jj_utf8_cancellable, run_jj_utf8_ignore_wc, run_jj_utf8_with_retry,
    run_jj_utf8_with_retry_cancellable, run_jj_utf8_with_timeout, run_jj_with_retry,
    run_jj_with_retry_cancellable, run_jj_with_timeout,
};

// Re-export procpilot's generic subprocess API so vcs-runner consumers have
// one dependency. Prefer these for anything non-VCS-specific.
pub use procpilot::{
    Cmd, CmdDisplay, DefaultRunner, Redirection, RetryPolicy, RunOutput, Runner,
    STREAM_SUFFIX_SIZE, SpawnedProcess, StdinData, binary_available, binary_version,
    default_transient,
};

pub use worktree::{read_working_file, read_working_file_bytes, working_file_is_binary};

pub use types::JjOperation;
#[cfg(any(feature = "jj-parse", feature = "git-parse"))]
pub use types::{FileChange, FileChangeKind};
#[cfg(feature = "jj-parse")]
pub use types::{Bookmark, ConflictState, ContentState, GitRemote, LogEntry, RemoteStatus, WorkingCopy};

/// Common types and helpers for everyday VCS subprocess work.
///
/// `use vcs_runner::prelude::*;` brings in procpilot's standard set
/// (`Cmd`, `RunError`, `RunOutput`, `Redirection`, `RetryPolicy`,
/// `StdinData`, `SpawnedProcess`) plus the VCS-specific helpers
/// (`run_jj`, `run_git`, retry/timeout variants, `jj_merge_base`,
/// `git_merge_base`, `is_transient_error`, and the binary-availability
/// checks).
///
/// Parser types and constants (`LogEntry`, `BOOKMARK_TEMPLATE`, …) stay
/// out of the prelude — those callers know they need them and can
/// import explicitly.
pub mod prelude {
    pub use procpilot::prelude::*;

    pub use crate::{
        VcsBackend, detect_vcs, git_available, git_merge_base, git_version, is_transient_error,
        jj_available, jj_current_operation_id, jj_divergent_change_ids,
        jj_is_divergent_at_operation, jj_merge_base, jj_op_restore, jj_operation_log,
        jj_revset_at_operation, jj_revset_history, jj_version,
        run_git, run_git_cancellable, run_git_utf8, run_git_utf8_cancellable,
        run_git_utf8_with_retry, run_git_utf8_with_retry_cancellable, run_git_utf8_with_timeout,
        run_git_with_retry, run_git_with_retry_cancellable, run_git_with_timeout, run_jj,
        run_jj_cancellable, run_jj_utf8, run_jj_utf8_cancellable, run_jj_utf8_ignore_wc,
        run_jj_utf8_with_retry, run_jj_utf8_with_retry_cancellable, run_jj_utf8_with_timeout,
        run_jj_with_retry, run_jj_with_retry_cancellable, run_jj_with_timeout,
        read_working_file, read_working_file_bytes, working_file_is_binary,
    };
}

/// Check whether the `jj` binary is available on PATH.
pub fn jj_available() -> bool {
    binary_available("jj")
}

/// Get the jj version string, if available.
pub fn jj_version() -> Option<String> {
    binary_version("jj")
}

/// Check whether the `git` binary is available on PATH.
pub fn git_available() -> bool {
    binary_available("git")
}

/// Get the git version string, if available.
pub fn git_version() -> Option<String> {
    binary_version("git")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jj_available_returns_bool() {
        let _ = jj_available();
    }

    #[test]
    fn jj_version_matches_availability() {
        if jj_available() {
            let v = jj_version().expect("jj is installed");
            assert!(v.contains("jj"));
        } else {
            assert!(jj_version().is_none());
        }
    }

    #[test]
    fn git_available_returns_bool() {
        let _ = git_available();
    }

    #[test]
    fn git_version_matches_availability() {
        if git_available() {
            let v = git_version().expect("git is installed");
            assert!(v.contains("git"));
        } else {
            assert!(git_version().is_none());
        }
    }
}
