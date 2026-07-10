# vcs-runner

Subprocess runner for [jj](https://jj-vcs.github.io/jj/) and git CLI tools, with automatic retry on transient errors, timeouts, repository detection, and structured output parsing for both VCS backends.

## Why not `std::process::Command`?

- **Typed errors** — distinguishes "couldn't spawn the binary" from "command ran and exited non-zero" from "timed out," so callers can handle each as appropriate
- **Retry with backoff** on lock contention and stale working copy errors
- **Timeout support** that kills hung commands (e.g., `git fetch` against an unreachable remote) and captures any partial output
- **Binary-safe output** (`Vec<u8>`) with convenient `.stdout_lossy()` for text
- **Repo detection** that walks parent directories and distinguishes git, jj, and colocated repos
- **Structured output parsing** (optional) for both backends — jj log, jj bookmarks, jj diff summary, git diff name-status
- **Merge-base helpers** for both backends with consistent `Option<String>` semantics

## Usage

```toml
[dependencies]
vcs-runner = "0.10"
```

### Cargo features

- `jj-parse` (default): enables jj output parsers (log, bookmarks, diff summary) — pulls in `serde` and `serde_json`
- `git-parse` (default): enables git output parsers (diff name-status) — no extra deps

Git-only consumers can skip jj parsing:

```toml
[dependencies]
vcs-runner = { version = "0.10", default-features = false, features = ["git-parse"] }
```

## Running commands

```rust
use vcs_runner::{run_jj, run_git, run_jj_with_retry, is_transient_error};

// Run a jj command, get captured output
let output = run_jj(&repo_path, &["log", "-r", "@", "--no-graph"])?;
let log_text = output.stdout_lossy();

// Binary content: access raw bytes directly (e.g., for image diffs)
let output = run_jj(&repo_path, &["file", "show", "-r", "@", "image.png"])?;
let image_bytes: Vec<u8> = output.stdout;

// With retry on lock contention / stale working copy
let output = run_jj_with_retry(&repo_path, &["diff", "--summary"], is_transient_error)?;

// Custom retry predicate receives a typed RunError
let output = run_jj_with_retry(&repo_path, &["status"], |err| {
    err.stderr().is_some_and(|s| s.contains("concurrent operation"))
})?;

// Git works the same way
let output = run_git(&repo_path, &["log", "--oneline", "-5"])?;
```

### Handling "command ran and said no"

`run_jj` and `run_git` return `Result<RunOutput, RunError>`. The `RunError` enum distinguishes infrastructure failure (binary missing, fork failed) from non-zero exits (the command ran and reported failure via exit code) from timeouts:

```rust
use vcs_runner::{run_git, RunError};

match run_git(&repo_path, &["show", "possibly-missing-ref"]) {
    Ok(output) => Some(output.stdout),
    Err(RunError::NonZeroExit { .. }) => None,   // ref doesn't exist — legitimate answer
    Err(e) => return Err(e.into()),              // real infrastructure failure
}
```

`RunError` implements `std::error::Error`, so `?` into `anyhow::Result` works when you don't care about the distinction.

Inspection methods on `RunError`:
- `err.is_non_zero_exit()` / `err.is_spawn_failure()` / `err.is_timeout()` / `err.is_cancelled()` — check the variant
- `err.stderr()` — captured stderr on `NonZeroExit`/`Timeout`/`Cancelled`, `None` on `Spawn`
- `err.exit_status()` — exit status on `NonZeroExit`, `None` on others
- `err.attempts()` — 1-based attempt count (relevant after `*_with_retry*`)
- `err.program()` — the program name that failed

`RunError` is marked `#[non_exhaustive]`, so new variants can be added in future versions without breaking your match arms (add a wildcard fallback).

### Timeouts

For commands that might hang (network ops, unreachable remotes, user-supplied revsets), use the timeout variants:

```rust
use std::time::Duration;
use vcs_runner::{run_git_with_timeout, RunError};

match run_git_with_timeout(&repo_path, &["fetch"], Duration::from_secs(30)) {
    Ok(_) => println!("fetched"),
    Err(RunError::Timeout { elapsed, stderr, .. }) => {
        eprintln!("fetch hung after {elapsed:?}; last stderr: {stderr}");
    }
    Err(e) => return Err(e.into()),
}
```

The timeout implementation drains stdout/stderr in background threads, so a chatty process can't block on pipe-buffer overflow. Any output collected before the kill is returned in the `Timeout` error variant.

**Caveat on grandchildren:** the kill signal reaches only the direct child. A shell wrapper like `sh -c "git fetch"` forks `git` as a grandchild that survives the shell's kill. Use `exec` in the shell (`sh -c "exec git fetch"`) or invoke `git` directly to avoid this.

### Cancellation

When a wall-clock timeout doesn't fit — e.g. a TUI event loop where the user
pressing `q` should immediately abort an in-flight `jj`/`git` call — thread an
`Arc<AtomicBool>` flag through the cancellable variants:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use vcs_runner::{run_jj_cancellable, RunError};

let cancel = Arc::new(AtomicBool::new(false));
let cancel_clone = Arc::clone(&cancel);

// UI thread: fire the flag the moment the operation is obsolete.
std::thread::spawn(move || {
    // ... wait for user to quit ...
    cancel_clone.store(true, Ordering::Relaxed);
});

match run_jj_cancellable(&repo_path, &["log", "-r", "all()"], cancel) {
    Ok(out) => println!("{}", out.stdout_lossy()),
    Err(RunError::Cancelled { .. }) => { /* user quit — exit cleanly */ }
    Err(e) => return Err(e.into()),
}
```

If the flag is already set when the helper is called, the child is never
spawned. If it fires mid-flight, the child receives SIGTERM (then SIGKILL
after a short grace) and the helper returns `RunError::Cancelled` with any
output captured before the kill.

Cancellation composes with retry — `run_jj_with_retry_cancellable` /
`run_git_with_retry_cancellable` short-circuit any pending backoff sleep
when the flag fires, and the default transient-error predicate does not
retry `Cancelled` (the caller asked to stop, so we stop). Each variant has
a `_utf8` counterpart that returns trimmed stdout as `String`.

### Commands other than jj/git

For any non-VCS subprocess, use [`Cmd`](https://docs.rs/procpilot/latest/procpilot/struct.Cmd.html) — re-exported from [`procpilot`](https://crates.io/crates/procpilot), so one `vcs-runner` dep covers both.

```rust
use std::time::Duration;
use vcs_runner::{Cmd, Redirection};

// Captured output with env, cwd, timeout — all composable.
let output = Cmd::new("make")
    .args(["test"])
    .in_dir(&repo_path)
    .env("CARGO_TARGET_DIR", "/tmp/target")
    .timeout(Duration::from_secs(60))
    .run()?;

// Pipe stdin into a child (kubectl apply -f -, docker build -, etc.)
Cmd::new("kubectl").args(["apply", "-f", "-"]).stdin(manifest_yaml).run()?;

// Let stderr stream to the user (live progress)
Cmd::new("cargo").args(["build"]).stderr(Redirection::Inherit).run()?;
```

## Repository detection

```rust
use vcs_runner::{detect_vcs, VcsBackend};

let (backend, root) = detect_vcs(&some_path)?;

if backend.is_jj() {
    // True for both Jj and Colocated
    let output = run_jj(&root, &["status"])?;
}

if backend.has_git() {
    // True for both Git and Colocated
    let output = run_git(&root, &["status"])?;
}
```

Detection walks parent directories automatically (e.g., `/repo/src/lib/` finds `/repo/.jj`).

## Merge base

Find the common ancestor of two revisions. Returns `Ok(None)` when there is no common ancestor (unrelated histories); `Err(_)` for actual failures like invalid refs.

```rust
use vcs_runner::{jj_merge_base, git_merge_base};

if let Some(base) = jj_merge_base(&repo, "trunk()", "@")? {
    println!("fork point: {base}");
}

if let Some(base) = git_merge_base(&repo, "origin/main", "HEAD")? {
    println!("fork point: {base}");
}
```

## Operation log

Helpers for jj's operation log — enough to detect and recover from a concurrent
op-log reconcile. When a second jj process mutates the same repo (say a
background watcher racing a foreground command), the operation log forks and jj
reconciles the two heads. jj **preserves both sides' commits**, so the only
corruption signature is a *divergent change* — two versions of one change,
produced when a rebase races the reconcile. That is the signal to gate on, and
`jj_divergent_change_ids` reports it directly.

The read helpers are **working-copy-agnostic** (`--ignore-working-copy`): they
never snapshot the user's in-progress edits, and — crucially — they stay
readable when a concurrent writer has left the working copy stale, which is
exactly when you need them (a plain read errors "working copy is stale" there).
Use `run_jj_utf8_ignore_wc` for your own reads, fetches, and pushes so they
don't perturb the user's checkout either.

```rust
use vcs_runner::{jj_current_operation_id, jj_divergent_change_ids, jj_op_restore};

// If the stack is already divergent, don't rebase it — gate and surface. Both
// versions are preserved in place; there is nothing to discard.
if !jj_divergent_change_ids(&repo)?.is_empty() {
    // ... report and retry later ...
}

// Capture the clean operation *after* fetch, before your rebase.
let post_fetch = jj_current_operation_id(&repo)?;

// ... run your rebase/merge ...

// If the rebase introduced divergence, it raced a concurrent reconcile. Undo
// ONLY your own step — restore to the op you captured — never roll back past
// the concurrent work; jj already preserved both sides.
if !jj_divergent_change_ids(&repo)?.is_empty() {
    jj_op_restore(&repo, &post_fetch)?;
}
```

Read the divergence signal **fail-safe**: an *error* reading it is usually lock
contention from the very concurrent writer you're guarding against, so treat it
as "can't verify — gate", never as "clean — proceed".

`jj_op_restore` is colocation-safe: jj re-exports refs to git as part of the
restore, so the git side (branch refs, `HEAD`) stays in lockstep with jj.
`jj_operation_log` and `jj_is_divergent_at_operation` remain available for
inspecting op history, but prefer `divergent()` as the corruption signal —
matching operation *descriptions* is fragile and flags benign independent
reconciles too. Deciding when to snapshot and what to restore to is application
policy; this crate provides the primitives.

## Parsing jj output

Requires the `jj-parse` feature (on by default). Pre-built templates produce line-delimited JSON; parse functions handle malformed output gracefully.

```rust
use vcs_runner::{run_jj, BOOKMARK_TEMPLATE, LOG_TEMPLATE};
use vcs_runner::{parse_bookmark_output, parse_log_output, parse_diff_summary};

// Log entries with structured fields
let output = run_jj(&repo, &[
    "log", "--revisions", "trunk()..@", "--no-graph", "--template", LOG_TEMPLATE,
])?;
let result = parse_log_output(&output.stdout_lossy());

for entry in &result.entries {
    println!("{} {}", entry.change_id, entry.summary());
    if entry.conflict.is_conflicted() {
        eprintln!("  has conflicts");
    }
}

// Bookmarks with sync status
let output = run_jj(&repo, &["bookmark", "list", "--template", BOOKMARK_TEMPLATE])?;
let result = parse_bookmark_output(&output.stdout_lossy());
for bookmark in &result.bookmarks {
    println!("{}: {:?}", bookmark.name, bookmark.remote);
}

// Diff summary — file changes between revisions
let output = run_jj(&repo, &["diff", "--from", "trunk()", "--to", "@", "--summary"])?;
for change in parse_diff_summary(&output.stdout_lossy()) {
    println!("{:?} {}", change.kind, change.path.display());
    if let Some(from) = &change.from_path {
        println!("  (renamed from {})", from.display());
    }
}
```

## Parsing git output

Requires the `git-parse` feature (on by default). No extra dependencies.

```rust
use vcs_runner::{run_git, parse_git_diff_name_status};

let output = run_git(&repo, &["diff", "--name-status", "origin/main", "HEAD"])?;
for change in parse_git_diff_name_status(&output.stdout_lossy()) {
    println!("{:?} {}", change.kind, change.path.display());
}
```

Both `parse_diff_summary` (jj) and `parse_git_diff_name_status` (git) return the same `Vec<FileChange>`, so tools that support both backends can share downstream logic.

## Binary availability

```rust
use vcs_runner::{jj_available, jj_version, git_available, binary_available};

if jj_available() {
    println!("{}", jj_version().unwrap());
}

// Generic: works with any binary that supports --version
if binary_available("mise") {
    // ...
}
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
