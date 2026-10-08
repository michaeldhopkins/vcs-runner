//! Parsers for the line-oriented jj output the operation-log helpers in
//! `runner.rs` read. Kept apart from the subprocess calls so they can be tested
//! and fuzzed without a `jj` binary. Ungated: none of this is JSON.

use crate::types::JjOperation;

/// Parse `jj op log -T 'id ++ "\t" ++ description.first_line() ++ "\n"'`.
///
/// Each line splits at its first tab: the id before it, the description after
/// (which may itself contain tabs). A line with no tab is not an entry.
pub(crate) fn parse_operation_log(out: &str) -> Vec<JjOperation> {
    out.lines()
        .filter_map(|line| {
            line.split_once('\t').map(|(id, desc)| JjOperation { id: id.to_string(), description: desc.to_string() })
        })
        .collect()
}

/// Parse output with one id per line (`-T 'commit_id ++ "\n"'` and the like),
/// dropping empty lines. Order and duplicates are kept.
pub(crate) fn parse_id_lines(out: &str) -> Vec<String> {
    out.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_log_splits_at_the_first_tab() {
        let ops = parse_operation_log("abc\tsnapshot working copy\ndef\tdescribe\twith tab\n");
        assert_eq!(
            ops,
            vec![
                JjOperation { id: "abc".into(), description: "snapshot working copy".into() },
                JjOperation { id: "def".into(), description: "describe\twith tab".into() },
            ]
        );
    }

    #[test]
    fn operation_log_skips_lines_without_a_tab() {
        assert!(parse_operation_log("no tab here\n\n").is_empty());
        assert_eq!(parse_operation_log("abc\t\n"), vec![JjOperation { id: "abc".into(), description: String::new() }]);
    }

    #[test]
    fn id_lines_drop_empty_lines_and_keep_order() {
        assert_eq!(parse_id_lines("b\n\na\nb\n"), vec!["b", "a", "b"]);
        assert!(parse_id_lines("").is_empty());
    }
}
