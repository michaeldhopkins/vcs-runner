mod detect;
#[cfg(feature = "jj-parse")]
mod parse;
mod runner;
mod types;

pub use detect::{VcsBackend, detect_vcs};
#[cfg(feature = "jj-parse")]
pub use parse::{
    BOOKMARK_TEMPLATE, LOG_TEMPLATE, BookmarkParseResult, LogParseResult, parse_bookmark_output,
    parse_log_output, parse_remote_list,
};
pub use runner::{
    RunOutput, binary_available, binary_version, is_transient_error, run_cmd, run_cmd_in,
    run_cmd_inherited, run_git, run_git_with_retry, run_jj, run_jj_with_retry, run_with_retry,
};
#[cfg(feature = "jj-parse")]
pub use types::{
    Bookmark, ConflictState, ContentState, GitRemote, LogEntry, RemoteStatus, WorkingCopy,
};

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
    fn jj_version_returns_option() {
        let version = jj_version();
        if jj_available() {
            let v = version.expect("should have version when jj is available");
            assert!(v.contains("jj"));
        } else {
            assert!(version.is_none());
        }
    }

    #[test]
    fn git_available_returns_bool() {
        let _ = git_available();
    }

    #[test]
    fn git_version_returns_option() {
        let version = git_version();
        if git_available() {
            let v = version.expect("should have version when git is available");
            assert!(v.contains("git"));
        } else {
            assert!(version.is_none());
        }
    }
}
