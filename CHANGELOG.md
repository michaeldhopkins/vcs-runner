# Changelog

All notable changes to vcs-runner are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.15.0] - 2026-07-10

### Features

- **`run_jj_utf8_ignore_wc`** — run a `jj` command with `--ignore-working-copy` prepended, returning trimmed stdout. The working-copy-agnostic entry point for any operation that must not perturb the user's checkout: a read that shouldn't snapshot their in-progress edits, or a fetch/push that never needs the working copy. Added to the prelude.

### Fixed

- **Op-log and divergence reads are now working-copy-agnostic.** `jj_current_operation_id`, `jj_operation_log`, `jj_divergent_change_ids`, and `jj_is_divergent_at_operation` previously shelled out without `--ignore-working-copy`, so each snapshotted the working copy — creating a spurious snapshot operation (perturbing the very op log being read) and, worse, erroring "working copy is stale" during a concurrent op-log reconcile, i.e. exactly the situation these helpers exist to detect. They now read working-copy-agnostically. `jj_op_restore` is unchanged (it mutates and legitimately updates the working copy).

### Documentation

- Rewrote the operation-log guide to gate on `divergent()` (jj preserves both sides of a reconcile, so a divergent change is the only corruption signature) rather than string-matching operation descriptions, and to restore only to your own captured post-fetch op rather than rolling back past concurrent work.

## [0.14.0] - 2026-07-09

### Features

- *(runner)* Jj operation-log helpers for divergence detection + recovery

### Miscellaneous

- Bump action-gh-release to v3

## [0.13.0] - 2026-05-18

### Breaking changes

- **procpilot dep bumped from 0.7 to 0.8.** procpilot added an `attempts: u32` field to the `RunError::NonZeroExit` and `RunError::Timeout` struct variants (the field also appears on the new `Cancelled` variant). Downstream code that destructures these variants by field name without `..` will fail to compile — add `attempts` to the pattern, switch to `..`, or prefer the `err.attempts()` accessor. Matches using `..` or wildcard arms, and the `is_*` / `attempts()` accessors, are unaffected.

### Features

- **`run_jj_cancellable` / `run_git_cancellable`** and their `_utf8`, `_with_retry`, and `_with_retry_utf8` siblings. Each takes an `Arc<AtomicBool>` cancel flag; when the flag fires the wrapper kills the child (SIGTERM → SIGKILL after procpilot's default grace) and returns the new `RunError::Cancelled` variant. A pre-set flag short-circuits before spawning the child. The retry variants short-circuit any pending backoff sleep, and the default transient-error predicate does not retry `Cancelled`. All eight helpers are added to the prelude. Motivated by TUI consumers (e.g. `branchdiff`) that need precise event-loop-driven cancellation of in-flight VCS calls, which wall-clock `timeout` cannot express. See [vcs-runner#1](https://github.com/michaeldhopkins/vcs-runner/issues/1) and [procpilot#1](https://github.com/michaeldhopkins/procpilot/issues/1) for design context.
- New `RunError` accessors surface automatically through the existing re-export: `is_cancelled()` and `attempts()`.

### Internal

- Re-export coverage test updated for procpilot 0.8.0 snapshot (no new top-level types — the new API surfaces through the existing `Cmd` and `RunError` re-exports).

## [0.12.1] - 2026-04-15

### Features

- **`run_jj_utf8` / `run_git_utf8`** — return lossy-decoded, trimmed stdout as `String` instead of `RunOutput`. Covers the most common call pattern for callers that treat subprocess stdout as text. Timeout and retry variants included: `run_jj_utf8_with_timeout`, `run_git_utf8_with_timeout`, `run_jj_utf8_with_retry`, `run_git_utf8_with_retry`. All added to the prelude.

### Internal

- `jj_merge_base` and `git_merge_base` now use the `_utf8` helpers internally, removing repeated `.stdout_lossy().trim().to_string()` calls.

## [0.12.0] - 2026-04-15

### Breaking changes

- **procpilot dep bumped from 0.6 to 0.7.** procpilot renamed `test-helpers` → `mock-binaries`. This doesn't affect vcs-runner consumers (internal feature), but if you depended on procpilot directly for that feature, update accordingly.

### Features

- Re-exports `procpilot::Runner` and `procpilot::DefaultRunner` — available via `vcs_runner::Runner` / `vcs_runner::DefaultRunner` (and the prelude). Downstream code that takes `&dyn Runner` can now be unit-tested with `procpilot::testing::MockRunner` without adding a separate procpilot dep for the trait itself.
- Re-export coverage test updated for procpilot 0.7.0 snapshot.

## [0.11.1] - 2026-04-15

### Tests

- New `tests/reexport_coverage.rs` audit: a compile-time check that every procpilot pub item we re-export resolves on both sides. Bumping the procpilot dep version is now a guided exercise — read procpilot's CHANGELOG, decide for each new item, update the snapshot.

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
