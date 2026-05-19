//! Compile-time audit of the items we re-export from procpilot.
//!
//! Maintenance discipline (when bumping the procpilot dep version):
//!   1. Read procpilot's CHANGELOG between the old and new version.
//!   2. For each new public item, decide: re-export it (and update both
//!      the upstream-side and our re-export lists below), or document why
//!      not in the "Intentionally not re-exported" block.
//!   3. Bump the version comment on the next line so future audits know
//!      the snapshot's vintage.
//!
//! What this catches:
//!   - procpilot removed an item we still claim to re-export
//!     (`upstream_surface` import fails to compile).
//!   - Our re-export typo (`our_reexports` import fails to compile).
//!
//! What it does NOT catch:
//!   - procpilot added a new item we should re-export but forgot. Step 2
//!     above is the only mitigation; this test trusts the author did it.

// Snapshot vintage: procpilot 0.8.0.
//   - 0.8.0 added Cmd::cancel / Cmd::cancel_grace methods, RunError::Cancelled
//     variant, RunError::is_cancelled / attempts accessors. No new top-level
//     types to re-export; existing Cmd / RunError re-exports surface the new
//     API automatically.

#[allow(unused_imports, dead_code)]
mod upstream_surface {
    pub use procpilot::{
        Cmd, CmdDisplay, DefaultRunner, Redirection, RetryPolicy, RunError, RunOutput, Runner,
        STREAM_SUFFIX_SIZE, SpawnedProcess, StdinData, binary_available, binary_version,
        default_transient,
    };
}

#[allow(unused_imports, dead_code)]
mod our_reexports {
    pub use vcs_runner::{
        Cmd, CmdDisplay, DefaultRunner, Redirection, RetryPolicy, RunError, RunOutput, Runner,
        STREAM_SUFFIX_SIZE, SpawnedProcess, StdinData, binary_available, binary_version,
        default_transient,
    };
}

// Intentionally not re-exported from procpilot:
//   - procpilot::AsyncSpawnedProcess: gated on procpilot's `tokio` feature,
//     which vcs-runner does not currently forward.
//   - procpilot::prelude: vcs-runner has its own prelude that includes
//     procpilot's via `pub use procpilot::prelude::*;`.
//   - procpilot::testing module (MockRunner, MockResult, helpers): gated
//     on procpilot's `testing` feature, which vcs-runner does not forward.
//     Downstream consumers who want mocks add procpilot directly with
//     features = ["testing"].

#[test]
fn reexport_audit_compiles() {}
