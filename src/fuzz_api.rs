//! Entry points for the fuzz targets in `fuzz/`, compiled only under `cargo fuzz`
//! (which passes `--cfg fuzzing`). The parsers they wrap are crate-private; this
//! module reaches them without making them part of the published library API.

use crate::types::JjOperation;

/// `jj op log` with the `id ++ "\t" ++ description.first_line()` template.
pub fn parse_operation_log(out: &str) -> Vec<JjOperation> {
    crate::parse_op::parse_operation_log(out)
}

/// One id per line (`divergent()` change ids, commit ids at an operation).
pub fn parse_id_lines(out: &str) -> Vec<String> {
    crate::parse_op::parse_id_lines(out)
}
