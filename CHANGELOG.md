# Changelog

All notable changes to vcs-runner are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.11.0] - 2026-04-15

### Breaking changes

- **procpilot dep bumped from 0.2 to 0.6.** Most of procpilot's 0.2 → 0.6.x changes are additive on the surface vcs-runner re-exports, but two transitively touch downstream code:
  - `RunOutput` is now `#[non_exhaustive]`. Downstream struct-literal construction (`RunOutput { stdout, stderr }`) won't compile; use `output.stdout` / `output.stderr` field access (unchanged).
  - `StdinData` is now `#[non_exhaustive]`. Downstream `match` on the enum needs a wildcard arm.
- **`BeforeSpawnHook` re-export removed.** procpilot dropped the public type alias; callers pass closures directly to `Cmd::before_spawn`.

### Features

- Re-exports `procpilot::SpawnedProcess` for spawn-handle access.
- New `vcs_runner::prelude` module — `use vcs_runner::prelude::*;` brings in procpilot's prelude plus the VCS helpers (`run_jj`, `run_git`, `*_with_timeout`, `*_with_retry`, `jj_merge_base`, `git_merge_base`, `is_transient_error`, `*_available`, `*_version`, `detect_vcs`, `VcsBackend`).
- All of procpilot 0.6's additions are reachable through vcs-runner: `Cmd::pipe` / `|` for pipelines, `Cmd::spawn` for `SpawnedProcess`, and (with procpilot's `tokio` feature) `Cmd::run_async` / `Cmd::spawn_async`.

## [0.10.0] - 2026-04-14

### Breaking changes

- **Generic subprocess primitives removed.** `run_cmd`, `run_cmd_in`, `run_cmd_in_with_env`, `run_cmd_in_with_timeout`, `run_cmd_inherited`, and `run_with_retry` are gone. Use [`procpilot::Cmd`] — re-exported as `vcs_runner::Cmd` — instead.
- **`RunError` is now `procpilot::RunError`.** Field shape changed: variants carry `command: CmdDisplay` instead of `program: String` + `args: Vec<String>`. Stdout/stderr on `NonZeroExit` / `Timeout` are truncated to the last 128 KiB.
- **Retry predicates now require `Send + Sync + 'static`** (procpilot's retry policy stores them in an `Arc`).
- Migration: `run_cmd_in(&dir, "git", &["status"])` → `Cmd::new("git").in_dir(&dir).args(["status"]).run()`. Error field `{ program, args }` → `{ command }`; `err.program()` still works.

### Changed

- `vcs-runner` now depends on [`procpilot`] for all subprocess execution. VCS-specific helpers (`run_jj`, `run_git`, `*_with_timeout`, `*_with_retry`, `jj_merge_base`, `git_merge_base`, `is_transient_error`) are preserved as thin wrappers.
- Re-exports `procpilot::{Cmd, CmdDisplay, Redirection, RetryPolicy, RunOutput, StdinData, binary_available, binary_version, default_transient, STREAM_SUFFIX_SIZE}` so consumers need only one dependency.
- `is_transient_error` now delegates to `procpilot::default_transient` (same semantics).

## [0.9.2] - 2026-04-14

### Miscellaneous

- Add project quality apparatus: `clippy.toml`, `cliff.toml`, `CLAUDE.md`, `scripts/stats.sh`, `examples/basic.rs`
- Set up `[package.metadata.docs.rs]` for clean docs.rs builds
- CI runs `cargo doc` with `RUSTDOCFLAGS=-D warnings` to catch broken doc links

## [0.9.1] - 2026-04-14

The 0.9 series was tagged-and-released as 0.9.1; the 0.9.0 push failed CI before publish (a test relied on the local default git branch name) and was never published to crates.io.

### Features

- Add `parse_diff_summary` for `jj diff --summary` output (behind `jj-parse` feature)
- Add `parse_git_diff_name_status` for `git diff --name-status` output (behind new `git-parse` feature, default-on)
- Add `jj_merge_base` and `git_merge_base` helpers; both return `Result<Option<String>>` with consistent semantics across backends
- Shared `FileChange` / `FileChangeKind` types usable with either parser

### Refactor

- Split `parse.rs` into `parse_jj.rs` and `parse_git.rs` for feature separation

### Bug Fixes

- Use explicit branch name in `git_merge_base` test for CI compatibility (CI defaults to `master`, local often defaults to `main`)

## [0.8.0] - 2026-04-14

### Features

- Add timeout support via `RunError::Timeout` variant and `run_*_with_timeout` functions
- Mark `RunError` as `#[non_exhaustive]` to allow future variants without breaking callers
- Background-thread pipe draining prevents deadlock on chatty processes that exceed the kill timeout

## [0.7.0] - 2026-04-14

### Breaking changes

- `RunError` is now a typed enum (`Spawn` / `NonZeroExit`) instead of `anyhow::Error`
- Distinguishes infrastructure failure (binary missing, fork failed) from non-zero exits (the command ran and reported failure)
- Retry predicate signature changed from `fn(&str)` to `impl Fn(&RunError)` for richer matching

## [0.6.1] - 2026-04-14

### Bug Fixes

- Tighten release workflow to only suppress 'already exists' errors (don't silently swallow other failures)

## [0.6.0] - 2026-04-14

### Features

- Add `run_cmd_in_with_env` for commands that need custom environment variables (e.g., `GIT_INDEX_FILE`)

## [0.5.0] - 2026-04-14

- Initial release as `vcs-runner` (renamed from `jj-runner` to reflect dual git+jj support)
