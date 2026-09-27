//! Which jj is on `PATH`, for the few queries whose spelling depends on it.
//!
//! vcs-runner supports jj 0.33 and later (see AGENTS.md "Supported jj versions").
//! Everything it runs is spelled the same across that range except where this
//! module says otherwise.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JjVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// jj 0.38 added the `divergent()` revset. Before it, the same set is the visible
/// commits whose `divergent` template keyword is true.
const DIVERGENT_REVSET_SINCE: JjVersion = JjVersion { major: 0, minor: 38, patch: 0 };

/// Parse `jj --version` output: `jj 0.45.1`, or `jj 0.33.0-24f4e10…` from a release
/// build. The first whitespace-separated token starting with a digit is the version;
/// anything after its third number (a `-hash` or `+dirty` suffix) is ignored.
pub(crate) fn parse_jj_version(out: &str) -> Option<JjVersion> {
    let token = out.split_whitespace().find(|t| t.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut numbers = token.split('.').map(|part| {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok()
    });
    Some(JjVersion { major: numbers.next()??, minor: numbers.next()??, patch: numbers.next()?? })
}

/// The version of the `jj` on `PATH`, read once per process. `None` when jj is
/// missing or prints something unrecognisable.
pub(crate) fn installed_jj_version() -> Option<JjVersion> {
    static VERSION: OnceLock<Option<JjVersion>> = OnceLock::new();
    *VERSION.get_or_init(|| procpilot::binary_version("jj").and_then(|v| parse_jj_version(&v)))
}

/// `jj log` arguments (`-r`, `-T` and their values) printing the change id of every
/// visible divergent commit, one per line. An unknown version gets the spelling
/// every supported version accepts.
pub(crate) fn divergent_change_id_query(version: Option<JjVersion>) -> [&'static str; 4] {
    if version.is_some_and(|v| v >= DIVERGENT_REVSET_SINCE) {
        ["-r", "divergent()", "-T", r#"change_id ++ "\n""#]
    } else {
        ["-r", "all()", "-T", r#"if(divergent, change_id ++ "\n")"#]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> JjVersion {
        JjVersion { major, minor, patch }
    }

    #[test]
    fn parses_release_and_plain_version_strings() {
        assert_eq!(parse_jj_version("jj 0.45.1"), Some(v(0, 45, 1)));
        assert_eq!(
            parse_jj_version("jj 0.33.0-24f4e1083e8bcd6e5b8aaee3fa86e08cb7081d13"),
            Some(v(0, 33, 0))
        );
        assert_eq!(parse_jj_version("jj 1.2.3+dirty\n"), Some(v(1, 2, 3)));
    }

    #[test]
    fn rejects_what_is_not_a_version() {
        assert_eq!(parse_jj_version(""), None);
        assert_eq!(parse_jj_version("jj"), None);
        assert_eq!(parse_jj_version("jj 0.45"), None);
        assert_eq!(parse_jj_version("jj 0.x.1"), None);
    }

    #[test]
    fn divergent_query_uses_the_revset_from_0_38() {
        assert_eq!(divergent_change_id_query(Some(v(0, 38, 0)))[1], "divergent()");
        assert_eq!(divergent_change_id_query(Some(v(1, 0, 0)))[1], "divergent()");
    }

    #[test]
    fn divergent_query_falls_back_before_0_38_and_when_unknown() {
        for version in [Some(v(0, 37, 9)), Some(v(0, 33, 0)), None] {
            let query = divergent_change_id_query(version);
            assert_eq!(query[1], "all()", "{version:?}");
            assert!(query[3].starts_with("if(divergent"), "{version:?}");
        }
    }
}
