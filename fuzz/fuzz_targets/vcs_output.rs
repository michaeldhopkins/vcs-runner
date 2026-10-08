#![no_main]

//! NEVER-PANICS target: every parser vcs-runner runs over jj's or git's stdout.
//!
//! `jj log` / `jj bookmark list` with `LOG_TEMPLATE` / `BOOKMARK_TEMPLATE`, `jj diff
//! --summary`, `jj git remote list`, `jj op log`, one-id-per-line revset output, and
//! `git diff --name-status`. None of these formats is vcs-runner's to version: a jj
//! upgrade can change them underneath every consumer, and descriptions and paths are
//! whatever the user typed. A panic here takes down the consumer (branchdiff, jjpr).
//!
//! Beyond not panicking, it holds each parser to the one bound that needs no
//! knowledge of the format: a line-oriented parser yields at most one record per
//! input line, and `parse_log_output` accounts for every non-blank line exactly once
//! (as an entry or as a skipped line), since a line it silently lost would be a
//! commit the caller never hears about.

use libfuzzer_sys::fuzz_target;

use vcs_runner::fuzz_api::{parse_id_lines, parse_operation_log};
use vcs_runner::{
    parse_bookmark_output, parse_diff_summary, parse_git_diff_name_status, parse_log_output, parse_remote_list,
};

fuzz_target!(|data: &[u8]| {
    // Consumers read jj/git stdout lossily (a path or description can hold any bytes),
    // so lossy here too; rejecting invalid UTF-8 would fuzz a path nobody takes.
    let text = String::from_utf8_lossy(data);
    let lines = text.lines().count();
    let non_blank = text.lines().filter(|l| !l.trim().is_empty()).count();
    let non_empty = text.lines().filter(|l| !l.is_empty()).count();

    let log = parse_log_output(&text);
    assert_eq!(log.entries.len() + log.skipped.len(), non_blank, "every non-blank line is an entry or a skipped line");

    let bookmarks = parse_bookmark_output(&text);
    assert!(bookmarks.bookmarks.len() + bookmarks.skipped.len() <= non_blank);

    assert!(parse_diff_summary(&text).len() <= lines);
    assert!(parse_git_diff_name_status(&text).len() <= lines);
    assert!(parse_remote_list(&text).len() <= lines);
    assert!(parse_operation_log(&text).len() <= lines);
    assert_eq!(parse_id_lines(&text).len(), non_empty);
});
