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
- `err.is_non_zero_exit()` / `err.is_spawn_failure()` / `err.is_timeout()` — check the variant
- `err.stderr()` — captured stderr on `NonZeroExit`/`Timeout`, `None` on `Spawn`
- `err.exit_status()` — exit status on `NonZeroExit`, `None` on others
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
