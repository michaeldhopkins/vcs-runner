use std::path::{Component, Path, PathBuf};

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
///
/// A relative path is taken from the working directory, and the walk goes on above
/// it. A path containing `..` is resolved through the filesystem first (so the root
/// comes back canonical), and one the filesystem cannot resolve, such as
/// `missing/..`, is an error.
pub fn detect_vcs(path: &Path) -> anyhow::Result<(VcsBackend, PathBuf)> {
    let cwd = if path.is_relative() { Some(std::env::current_dir()?) } else { None };
    let path = resolve(path, cwd.as_deref())?;
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

/// Where `path` really is, in a form whose lexical ancestors are its real ones.
///
/// `Path::ancestors` only strips components, so walking `sub/../other` visits `sub`,
/// which `other` is not inside, and walking a relative path stops at the working
/// directory. So the path is made absolute against `cwd`, and everything up to its
/// last `..` is canonicalized; what follows has no `..` and may not exist yet.
fn resolve(path: &Path, cwd: Option<&Path>) -> anyhow::Result<PathBuf> {
    let absolute = match cwd {
        Some(cwd) if path.is_relative() => cwd.join(path),
        _ => path.to_path_buf(),
    };
    let components: Vec<Component> = absolute.components().collect();
    let Some(last_up) = components.iter().rposition(|c| matches!(c, Component::ParentDir)) else {
        return Ok(components.iter().collect());
    };
    let head: PathBuf = components[..=last_up].iter().collect();
    let head = head
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("cannot resolve {}: {e}", head.display()))?;
    Ok(components[last_up + 1..].iter().fold(head, |p, c| p.join(c)))
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

    // The next three were found by the detect_root fuzz target. `Path::ancestors` is
    // lexical, so an ancestor walk over an unresolved path visits directories the path
    // is not inside.

    #[test]
    fn detect_through_dotdot_does_not_credit_the_dir_it_left() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let sub = tmp.path().join("sub");
        fs::create_dir_all(sub.join(".jj")).expect("mkdir sub/.jj");
        fs::create_dir(tmp.path().join("other")).expect("mkdir other");
        // `sub/../other` is `other`, which is not inside `sub`.
        if let Ok((_, root)) = detect_vcs(&sub.join("..").join("other")) {
            assert!(!root.starts_with(&sub), "credited {} to a path outside it", root.display());
        }
    }

    #[test]
    fn detect_through_dotdot_finds_the_real_parent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(tmp.path().join(".git")).expect("mkdir .git");
        let sub = tmp.path().join("sub");
        fs::create_dir_all(sub.join(".jj")).expect("mkdir sub/.jj");
        let (backend, root) = detect_vcs(&sub.join("..")).expect("should detect");
        assert_eq!(backend, VcsBackend::Git);
        assert_eq!(root, tmp.path().canonicalize().expect("canonical"));
        assert!(detect_vcs(&sub.join("missing").join("..")).is_err(), "unresolvable, as for the OS");
    }

    #[test]
    fn a_relative_path_is_resolved_against_the_working_directory() {
        let cwd = Path::new("/repo/sub");
        assert_eq!(resolve(Path::new(""), Some(cwd)).expect("resolves"), PathBuf::from("/repo/sub"));
        assert_eq!(resolve(Path::new("."), Some(cwd)).expect("resolves"), PathBuf::from("/repo/sub"));
        assert_eq!(resolve(Path::new("a/./b"), Some(cwd)).expect("resolves"), PathBuf::from("/repo/sub/a/b"));
        assert_eq!(resolve(Path::new("/abs"), Some(cwd)).expect("resolves"), PathBuf::from("/abs"));
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
