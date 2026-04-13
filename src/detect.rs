use std::path::{Path, PathBuf};

/// Which version control system is managing a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VcsBackend {
    /// Pure git repository (.git/ only)
    Git,
    /// Pure jj repository (.jj/ only)
    Jj,
    /// Colocated: both .jj/ and .git/ exist
    Colocated,
}

impl VcsBackend {
    /// Whether this backend uses jj (true for both `Jj` and `Colocated`).
    pub fn is_jj(self) -> bool {
        matches!(self, Self::Jj | Self::Colocated)
    }

    /// Whether this backend has a git repository (true for both `Git` and `Colocated`).
    pub fn has_git(self) -> bool {
        matches!(self, Self::Git | Self::Colocated)
    }
}

/// Detect the VCS backend for a path, without running any subprocesses.
///
/// Checks the path itself, then walks up ancestors. Returns the backend
/// type and the repo root path.
pub fn detect_vcs(path: &Path) -> anyhow::Result<(VcsBackend, PathBuf)> {
    for dir in path.ancestors() {
        let has_jj = dir.join(".jj").is_dir();
        let has_git = dir.join(".git").exists();

        match (has_jj, has_git) {
            (true, true) => return Ok((VcsBackend::Colocated, dir.to_path_buf())),
            (true, false) => return Ok((VcsBackend::Jj, dir.to_path_buf())),
            (false, true) => return Ok((VcsBackend::Git, dir.to_path_buf())),
            (false, false) => continue,
        }
    }
    anyhow::bail!("not a git or jj repository")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detect_empty_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(detect_vcs(tmp.path()).is_err());
    }

    #[test]
    fn detect_jj_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(tmp.path().join(".jj")).expect("mkdir .jj");
        let (backend, root) = detect_vcs(tmp.path()).expect("should detect");
        assert_eq!(backend, VcsBackend::Jj);
        assert_eq!(root, tmp.path());
        assert!(backend.is_jj());
        assert!(!backend.has_git());
    }

    #[test]
    fn detect_git_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(tmp.path().join(".git")).expect("mkdir .git");
        let (backend, root) = detect_vcs(tmp.path()).expect("should detect");
        assert_eq!(backend, VcsBackend::Git);
        assert_eq!(root, tmp.path());
        assert!(!backend.is_jj());
        assert!(backend.has_git());
    }

    #[test]
    fn detect_colocated() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(tmp.path().join(".jj")).expect("mkdir .jj");
        fs::create_dir(tmp.path().join(".git")).expect("mkdir .git");
        let (backend, _) = detect_vcs(tmp.path()).expect("should detect");
        assert_eq!(backend, VcsBackend::Colocated);
        assert!(backend.is_jj());
        assert!(backend.has_git());
    }

    #[test]
    fn detect_ancestor() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(tmp.path().join(".jj")).expect("mkdir .jj");
        let child = tmp.path().join("subdir");
        fs::create_dir(&child).expect("mkdir subdir");
        let (backend, root) = detect_vcs(&child).expect("should detect");
        assert_eq!(backend, VcsBackend::Jj);
        assert_eq!(root, tmp.path());
    }

    #[test]
    fn detect_git_worktree_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join(".git"), "gitdir: /other/.git/worktrees/wt")
            .expect("write .git file");
        let (backend, _) = detect_vcs(tmp.path()).expect("should detect");
        assert_eq!(backend, VcsBackend::Git);
    }
}
