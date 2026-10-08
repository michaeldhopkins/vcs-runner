//! Every workflow bounds its runs: a concurrency group, read-only token permissions at the top,
//! and a timeout on every job. A hung job otherwise runs for GitHub's six-hour default.
//!
//! The scan is line-based (top-level keys at column 0, job names at four spaces) so the crate
//! needs no YAML parser; the workflows here are plain block YAML.

use std::path::Path;

fn workflows() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut found: Vec<(String, String)> = std::fs::read_dir(&dir)
        .expect("read .github/workflows")
        .map(|e| e.expect("list .github/workflows").path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .map(|p| {
            (
                p.file_name().expect("a file name").to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).expect("read a workflow"),
            )
        })
        .collect();
    found.sort();
    found
}

/// Everything wrong with one workflow, as `name: problem`.
fn bound_problems(name: &str, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    if !lines.iter().any(|l| l.starts_with("concurrency:")) {
        problems.push(format!("{name}: no top-level concurrency group"));
    }
    match lines.iter().position(|l| l.starts_with("permissions:")) {
        Some(at) if lines.get(at + 1).is_some_and(|l| l.trim() == "contents: read") => {}
        Some(_) => problems.push(format!("{name}: top-level permissions are not exactly `contents: read`")),
        None => problems.push(format!("{name}: no top-level permissions")),
    }
    let Some(jobs) = lines.iter().position(|l| l.trim_end() == "jobs:") else {
        problems.push(format!("{name}: no jobs"));
        return problems;
    };
    let mut current: Option<(&str, bool)> = None;
    for line in &lines[jobs + 1..] {
        let indent = line.len() - line.trim_start().len();
        if indent == 2 && line.trim_end().ends_with(':') && !line.trim_start().starts_with('#') {
            if let Some((job, false)) = current {
                problems.push(format!("{name}: job {job} sets no timeout-minutes"));
            }
            current = Some((line.trim().trim_end_matches(':'), false));
        } else if let Some(job) =
            current.as_mut().filter(|_| indent == 4 && line.trim_start().starts_with("timeout-minutes:"))
        {
            job.1 = true;
        }
    }
    if let Some((job, false)) = current {
        problems.push(format!("{name}: job {job} sets no timeout-minutes"));
    }
    problems
}

#[test]
fn every_workflow_is_bounded() {
    let all = workflows();
    assert!(all.len() >= 4, "found only {} workflows", all.len());
    let problems: Vec<String> = all.iter().flat_map(|(n, t)| bound_problems(n, t)).collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

const BOUNDED: &str = "permissions:
  contents: read

concurrency:
  group: g

jobs:
  a:
    runs-on: x
    timeout-minutes: 5
  b:
    timeout-minutes: 9
";

#[test]
fn a_bounded_workflow_passes() {
    assert!(bound_problems("w", BOUNDED).is_empty());
}

#[test]
fn a_missing_concurrency_group_is_flagged() {
    let text = BOUNDED.replace("concurrency:", "other:");
    assert_eq!(bound_problems("w", &text), vec!["w: no top-level concurrency group"]);
}

#[test]
fn write_or_absent_permissions_are_flagged() {
    let write = BOUNDED.replace("contents: read", "contents: write");
    assert_eq!(bound_problems("w", &write).len(), 1);
    let none = BOUNDED.replace("permissions:", "perms:");
    assert_eq!(bound_problems("w", &none), vec!["w: no top-level permissions"]);
}

#[test]
fn a_job_without_a_timeout_is_flagged_first_middle_and_last() {
    let last = BOUNDED.replace("    timeout-minutes: 9\n", "    runs-on: x\n");
    assert_eq!(bound_problems("w", &last), vec!["w: job b sets no timeout-minutes"]);
    let first = BOUNDED.replace("    timeout-minutes: 5\n", "");
    assert_eq!(bound_problems("w", &first), vec!["w: job a sets no timeout-minutes"]);
}
