# Changelog

All notable changes to vcs-runner are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.19.1] - 2026-10-08

### Bug Fixes

- `jj_merge_base` returns `Ok(None)` when the only common ancestor is jj's `root()`, the all-zero commit id, as `git_merge_base` and the documentation already said. It used to return `Some` of forty zeros for revisions that share no history.

## [0.19.0] - 2026-10-08

### Breaking

- **procpilot 0.9.** The re-exported `Cmd`, `RunError`, `SpawnedProcess` and the rest now come from procpilot 0.9, so a crate that also depends on procpilot directly needs 0.9 for the types to match. Dropping a `SpawnedProcess` now kills and reaps every stage still running, as `AsyncSpawnedProcess` already did. To let a child outlive its handle, spawn it with `std::process::Command`.

### Bug Fixes

From procpilot 0.9:

- `run_jj*` and `run_git*` resolve `jj` and `git` through `PATH` once, then spawn them by path.
- With the re-exported `Cmd`, a one-shot `StdinData::Reader` taken by an earlier attempt now leaves a retry an empty stdin, as documented. A retry used to inherit the parent's stdin, where a child that reads stdin could wait forever.
- `CmdDisplay`, used in every `RunError` message, quotes a program that a shell would read as an assignment or a reserved word.

## [0.18.0] - 2026-09-26

Supports jj 0.33 and later, tested against real output from jj 0.33.0, 0.36.0, 0.37.0, 0.38.0, 0.40.0 and 0.45.1 (see "Supported jj versions" in the README).

### Breaking

- **`detect_vcs` resolves the path before walking up.** A relative path is taken from the working directory, and the walk continues above it: `detect_vcs(Path::new("."))` from a repo subdirectory used to return an error. A path containing `..` is canonicalized up to its last `..`, so the returned root is canonical, and a `..` the filesystem cannot resolve (`missing/..`) is an error. Before, `sub/../other` returned `sub`'s repository. Absolute paths without `..` return the same root as before.
- **`BOOKMARK_TEMPLATE` output has changed.** It adds `remote` and replaces `remoteBookmarks` with `remoteRefs` (raw `[name, remote]` pairs). `parse_bookmark_output` still reads the old output, using the old rule. Code that parses `BOOKMARK_TEMPLATE` output itself needs updating.

### Bug Fixes

- **Bookmarks.** jj prints one bookmark-list line per ref, including each tracked remote bookmark that points elsewhere. Every such line used to come back as an extra bookmark. Status came from whichever remote bookmarks shared the local target, so a bookmark whose remote had moved read as `Local`, and one sharing a commit with another bookmark's remote read as `Unsynced`. Now only local bookmarks are reported:
  - `Unsynced` when a remote of theirs points elsewhere or is conflicted;
  - `Synced` when their own remote is on their target;
  - `Local` otherwise.

  A deleted or conflicted bookmark's name is decoded properly in `skipped`.
- **Divergence on older jj.** `jj_divergent_change_ids` and `jj_is_divergent_at_operation` failed on jj before 0.38, which has no `divergent()` revset. They now check `jj --version` and use an equivalent query there.
- **Renames and copies.** `parse_diff_summary` dropped every rename and copy: jj prints `R src/{a.rs => b.rs}`, not `R a -> b`. The brace form is now decoded.
- **git paths.** `parse_git_diff_name_status` returned quoted paths (git's default `core.quotePath`, e.g. `"\303\251.rs"`) with their quotes. They are now unquoted.
- **Trailing whitespace.** Both diff parsers trimmed trailing whitespace off paths.

### Testing

- Five cargo-fuzz targets over every output parser and `detect_vcs`, replayed on each push and PR and burst on each push to main.
- Per-jj-version fixture tests, and a CI job that runs the suite against six jj releases.

## [0.17.0] - 2026-07-11

### Features

- *(worktree)* Read working-tree files from disk without snapshotting

## [0.16.0] - 2026-07-10

### Features

- **`jj_revset_history` / `jj_revset_at_operation`** — reconstruct what a revset resolved to over the operation log, the first-class replacement for scraping `op log --op-diff` prose (jj exposes no structured op-diff, so text-scraping is the alternative — and it is exactly the "op log is a safety net, not a tool" anti-pattern). `jj_revset_at_operation(repo, revset, op)` returns the commit ids a revset resolves to at one operation, wrapping it in `present(…)` so an operation older than the named ref yields an empty vec instead of erroring. `jj_revset_history(repo, revset, limit)` walks operations newest-first and returns the distinct commit ids the revset resolved to — jj's analog of `git reflog <ref>` — stopping at the first operation where the revset is absent, which bounds the walk to the ref's own lifetime (`limit`, `0` = none, is an extra cap for large logs). Both are working-copy-agnostic. Uses: working-copy / stranded-work recovery (`<ws>@` history), pre-rebase-head recovery (`main` history), and drift detection. The stop-at-absent bound suits ref-like revsets; for transiently-empty ones (`divergent()`, `conflicts()`) evaluate `jj_revset_at_operation` over an explicit op list. Both added to the prelude.

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
