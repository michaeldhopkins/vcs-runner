#![no_main]

//! ROUNDTRIP target: a list of file changes, rendered the way `jj diff --summary` and
//! `git diff --name-status` print it, parses back to exactly that list.
//!
//! **jj** prints `M path`, and for a rename or copy factors the shared leading and
//! trailing path components out into a brace group: `R src/{a.rs => b.rs}`,
//! `R {d => g}/e/x.rs`, `R g/{e => }/x.rs` (captured from jj 0.45). How much jj factors
//! out is its choice, so the target fuzzes it: any factoring of a real common prefix
//! and suffix must decode to the same two paths. The older `R old -> new` spelling is
//! rendered too, since the parser still accepts it. jj does not quote paths, so the
//! frame keeps what would make a jj line ambiguous out of the path: no line break, and,
//! in a rename or copy, no `{`, `}` or `>` (so no ` => ` or ` -> `). Anything else
//! stays, trailing spaces included.
//!
//! **git** separates fields with tabs and, as `core.quotePath` does by default, quotes a
//! path holding a control character, `"`, `\` or (unless the fuzzed `quote_path` is off)
//! any non-ASCII byte, as a C string with octal escapes. So the git half keeps every
//! character: the quoting is part of what it tests.

use std::path::PathBuf;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use vcs_runner::{FileChange, FileChangeKind, parse_diff_summary, parse_git_diff_name_status};

#[derive(Arbitrary, Debug, Clone, Copy)]
enum Kind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
}

#[derive(Arbitrary, Debug)]
struct Change {
    kind: Kind,
    path: Vec<String>,
    from: Vec<String>,
    /// How many of the common leading / trailing components jj factors out.
    prefix_factor: u8,
    suffix_factor: u8,
    /// Render the older `old -> new` spelling instead of a brace group.
    arrow: bool,
    score: Option<u8>,
}

#[derive(Arbitrary, Debug)]
struct Input {
    changes: Vec<Change>,
    quote_path: bool,
}

impl Kind {
    fn file_change_kind(self) -> FileChangeKind {
        match self {
            Kind::Modified => FileChangeKind::Modified,
            Kind::Added => FileChangeKind::Added,
            Kind::Deleted => FileChangeKind::Deleted,
            Kind::Renamed => FileChangeKind::Renamed,
            Kind::Copied => FileChangeKind::Copied,
        }
    }
    fn letter(self) -> char {
        match self {
            Kind::Modified => 'M',
            Kind::Added => 'A',
            Kind::Deleted => 'D',
            Kind::Renamed => 'R',
            Kind::Copied => 'C',
        }
    }
    fn is_pair(self) -> bool {
        matches!(self, Kind::Renamed | Kind::Copied)
    }
}

/// Path components with `forbidden` characters removed, empty components dropped, and
/// never an empty path.
fn segments(raw: &[String], forbidden: &[char]) -> Vec<String> {
    let mut segs: Vec<String> = raw
        .iter()
        .map(|s| s.chars().filter(|c| *c != '/' && !forbidden.contains(c)).collect::<String>())
        .filter(|s| !s.is_empty())
        .collect();
    if segs.is_empty() {
        segs.push("f".to_string());
    }
    segs
}

/// A rename's two sides, made different so it is a rename at all.
fn pair(c: &Change, forbidden: &[char]) -> (Vec<String>, Vec<String>) {
    let from = segments(&c.from, forbidden);
    let mut to = segments(&c.path, forbidden);
    if to == from {
        if let Some(last) = to.last_mut() {
            last.push('2');
        }
    }
    (from, to)
}

fn common_prefix(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

fn common_suffix(a: &[String], b: &[String]) -> usize {
    a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count()
}

fn jj_pair(from: &[String], to: &[String], c: &Change) -> String {
    if c.arrow {
        return format!("{} -> {}", from.join("/"), to.join("/"));
    }
    let max_p = common_prefix(from, to);
    let p = usize::from(c.prefix_factor) % (max_p + 1);
    let max_s = common_suffix(&from[p..], &to[p..]);
    let s = usize::from(c.suffix_factor) % (max_s + 1);
    let prefix = if p == 0 { String::new() } else { format!("{}/", from[..p].join("/")) };
    let suffix = if s == 0 { String::new() } else { format!("/{}", from[from.len() - s..].join("/")) };
    let old_mid = from[p..from.len() - s].join("/");
    let new_mid = to[p..to.len() - s].join("/");
    format!("{prefix}{{{old_mid} => {new_mid}}}{suffix}")
}

fn git_quote(path: &str, quote_path: bool) -> String {
    let special = |b: u8| b < 0x20 || b == 0x7f || b == b'"' || b == b'\\' || (quote_path && b >= 0x80);
    if !path.bytes().any(special) {
        return path.to_string();
    }
    let mut out: Vec<u8> = vec![b'"'];
    for b in path.bytes() {
        let named = match b {
            0x07 => Some(b'a'),
            0x08 => Some(b'b'),
            b'\t' => Some(b't'),
            b'\n' => Some(b'n'),
            0x0b => Some(b'v'),
            0x0c => Some(b'f'),
            b'\r' => Some(b'r'),
            b'"' => Some(b'"'),
            b'\\' => Some(b'\\'),
            _ => None,
        };
        if let Some(n) = named {
            out.extend([b'\\', n]);
        } else if special(b) {
            out.extend(format!("\\{b:03o}").bytes());
        } else {
            out.push(b);
        }
    }
    out.push(b'"');
    String::from_utf8(out).expect("unescaped bytes are whole UTF-8 sequences")
}

fn change(kind: Kind, path: &[String], from: Option<&[String]>) -> FileChange {
    FileChange {
        kind: kind.file_change_kind(),
        path: PathBuf::from(path.join("/")),
        from_path: from.map(|f| PathBuf::from(f.join("/"))),
    }
}

fn check_jj(changes: &[Change]) {
    let mut out = String::new();
    let mut want = Vec::new();
    for c in changes {
        if c.kind.is_pair() {
            let (from, to) = pair(c, &['\n', '\r', '{', '}', '>']);
            out.push_str(&format!("{} {}\n", c.kind.letter(), jj_pair(&from, &to, c)));
            want.push(change(c.kind, &to, Some(&from)));
        } else {
            let path = segments(&c.path, &['\n', '\r']);
            out.push_str(&format!("{} {}\n", c.kind.letter(), path.join("/")));
            want.push(change(c.kind, &path, None));
        }
    }
    assert_eq!(parse_diff_summary(&out), want, "jj output:\n{out}");
}

fn check_git(changes: &[Change], quote_path: bool) {
    let mut out = String::new();
    let mut want = Vec::new();
    for c in changes {
        let status = match (c.kind.is_pair(), c.score) {
            (true, Some(score)) => format!("{}{:03}", c.kind.letter(), score % 101),
            _ => c.kind.letter().to_string(),
        };
        if c.kind.is_pair() {
            let (from, to) = pair(c, &[]);
            let q = |p: &[String]| git_quote(&p.join("/"), quote_path);
            out.push_str(&format!("{status}\t{}\t{}\n", q(&from), q(&to)));
            want.push(change(c.kind, &to, Some(&from)));
        } else {
            let path = segments(&c.path, &[]);
            out.push_str(&format!("{status}\t{}\n", git_quote(&path.join("/"), quote_path)));
            want.push(change(c.kind, &path, None));
        }
    }
    assert_eq!(parse_git_diff_name_status(&out), want, "git output:\n{out}");
}

fuzz_target!(|input: Input| {
    check_jj(&input.changes);
    check_git(&input.changes, input.quote_path);
});
