//! Verifies the `prelude` module exports the items advertised in its
//! doc-comment. Compiled as a downstream consumer would.

use vcs_runner::prelude::*;

#[allow(dead_code)]
fn _exports_procpilot(
    _: Cmd,
    _: RunError,
    _: RunOutput,
    _: Redirection,
    _: RetryPolicy,
    _: StdinData,
    _: SpawnedProcess,
) {
}

#[allow(dead_code)]
fn _exports_vcs(_: VcsBackend) {
    let _: fn() -> bool = jj_available;
    let _: fn() -> bool = git_available;
    let _: fn() -> Option<String> = jj_version;
    let _: fn() -> Option<String> = git_version;
}

#[test]
fn prelude_compiles() {}
