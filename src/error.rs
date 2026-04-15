//! Re-export of [`procpilot::RunError`].
//!
//! Historically vcs-runner defined its own error enum. Starting with 0.10.0
//! we delegate to procpilot for the generic subprocess-failure shape and
//! only layer VCS-specific helpers on top.

pub use procpilot::RunError;
