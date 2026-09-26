#![no_main]

//! INVARIANT target: `detect_vcs` finds the repository that actually contains the path,
//! and never answers with a directory the path is not inside.
//!
//! Builds a real directory chain `tmp/d0/d1/d2/d3/d4`, with a fuzzed marker at each
//! level (`.jj/`, `.git/`, a `.git` file as in a worktree, both, or a `.jj` *file*,
//! which marks nothing), then asks about a path walked from a fuzzed start through
//! fuzzed steps: down a level, `..`, `.`, or into a directory that does not exist. The
//! path is passed absolute, or relative to a fuzzed working directory inside the
//! chain.
//!
//! The oracle follows the steps the way the filesystem resolves them, then takes the
//! nearest marked level at or above that location: the answer must be exactly that
//! directory (compared canonically) with exactly that backend. With no marked level,
//! the answer must be an error or a directory above `tmp`, i.e. outside the frame. A
//! `..` out of a directory that does not exist cannot be resolved by the filesystem
//! either, so it must be an error. A `..` above `tmp` is not taken, which keeps every
//! path inside the frame without discarding the input.
//!
//! The working directory is process-wide, and libFuzzer writes crash artifacts to a
//! path relative to it, so it is restored before any assertion can fail.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use vcs_runner::{VcsBackend, detect_vcs};

const DEPTH: usize = 5;

#[derive(Arbitrary, Debug, Clone, Copy, PartialEq)]
enum Marker {
    None,
    JjDir,
    GitDir,
    GitFile,
    Both,
    JjFile,
}

#[derive(Arbitrary, Debug, Clone, Copy)]
enum Step {
    Down,
    Up,
    Dot,
    Missing,
}

#[derive(Arbitrary, Debug)]
struct Input {
    markers: [Marker; DEPTH],
    /// Chain depth (0 = `tmp`) to start from and, when `relative`, to make the working
    /// directory.
    start: u8,
    relative: bool,
    steps: Vec<Step>,
}

impl Marker {
    fn backend(self) -> Option<VcsBackend> {
        match self {
            Marker::None | Marker::JjFile => None,
            Marker::JjDir => Some(VcsBackend::Jj),
            Marker::GitDir | Marker::GitFile => Some(VcsBackend::Git),
            Marker::Both => Some(VcsBackend::Colocated),
        }
    }
}

fn original_cwd() -> &'static PathBuf {
    static CWD: OnceLock<PathBuf> = OnceLock::new();
    CWD.get_or_init(|| std::env::current_dir().expect("a working directory"))
}

/// `tmp` followed by the first `depth` chain directories.
fn level(tmp: &Path, depth: usize) -> PathBuf {
    (0..depth).fold(tmp.to_path_buf(), |p, i| p.join(format!("d{i}")))
}

fn build(tmp: &Path, markers: &[Marker; DEPTH]) {
    std::fs::create_dir_all(level(tmp, DEPTH)).expect("mkdir chain");
    for (i, m) in markers.iter().enumerate() {
        let dir = level(tmp, i + 1);
        let mkdir = |name: &str| std::fs::create_dir(dir.join(name)).expect("mkdir marker");
        let touch = |name: &str| std::fs::write(dir.join(name), "gitdir: elsewhere\n").expect("write marker");
        match m {
            Marker::None => {}
            Marker::JjDir => mkdir(".jj"),
            Marker::GitDir => mkdir(".git"),
            Marker::GitFile => touch(".git"),
            Marker::Both => {
                mkdir(".jj");
                mkdir(".git");
            }
            Marker::JjFile => touch(".jj"),
        }
    }
}

/// The relative path the steps spell, and where the filesystem resolves it: the chain
/// depth reached, or `None` if a `..` left a directory that does not exist.
fn walk(start: usize, steps: &[Step]) -> (PathBuf, Option<usize>) {
    let mut path = PathBuf::new();
    let mut depth = start;
    let mut missing = 0usize;
    let mut unresolvable = false;
    for step in steps.iter().take(16) {
        match step {
            Step::Down if missing == 0 && depth < DEPTH => {
                path.push(format!("d{depth}"));
                depth += 1;
            }
            Step::Down | Step::Missing => {
                path.push("missing");
                missing += 1;
            }
            Step::Up if missing > 0 => {
                path.push("..");
                unresolvable = true;
                missing -= 1;
            }
            Step::Up if depth > 0 => {
                path.push("..");
                depth -= 1;
            }
            Step::Up => {}
            Step::Dot => path.push("."),
        }
    }
    (path, if unresolvable { None } else { Some(depth) })
}

fuzz_target!(|input: Input| {
    let home = original_cwd();
    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let tmp = tmp_dir.path().canonicalize().expect("canonical tmp");
    build(&tmp, &input.markers);

    let start = usize::from(input.start) % (DEPTH + 1);
    let (steps, resolved) = walk(start, &input.steps);

    let result = if input.relative {
        std::env::set_current_dir(level(&tmp, start)).expect("chdir");
        let r = detect_vcs(&steps);
        // Canonicalize while the working directory still gives a relative root meaning.
        let r = r.map(|(b, root)| (b, root.canonicalize()));
        std::env::set_current_dir(home).expect("restore working directory");
        r
    } else {
        detect_vcs(&level(&tmp, start).join(&steps)).map(|(b, root)| (b, root.canonicalize()))
    };

    let Some(depth) = resolved else {
        assert!(result.is_err(), "`..` out of a missing directory resolved to {result:?}");
        return;
    };
    let expected = (1..=depth)
        .rev()
        .find_map(|d| input.markers[d - 1].backend().map(|b| (b, level(&tmp, d))));

    match (expected, result) {
        (Some((backend, root)), Ok((got_backend, got_root))) => {
            let got_root = got_root.expect("the answer exists");
            assert_eq!(got_root, root, "wrong root for {steps:?} from depth {start}");
            assert_eq!(got_backend, backend, "wrong backend at {}", root.display());
        }
        (Some((_, root)), Err(e)) => panic!("missed the repo at {}: {e}", root.display()),
        (None, Ok((_, got_root))) => {
            let got_root = got_root.expect("the answer exists");
            assert!(
                tmp.starts_with(&got_root) && got_root != tmp,
                "no marked level contains the path, yet it answered {}",
                got_root.display()
            );
        }
        (None, Err(_)) => {}
    }
});
