# vcs-runner

Subprocess runner for [jj](https://jj-vcs.github.io/jj/) and git CLI tools, with automatic retry on transient errors, repository detection, and structured jj output parsing.

## Why not `std::process::Command`?

- **Typed errors** — distinguishes "couldn't spawn the binary" from "command ran and exited non-zero," so callers can handle the second as a legitimate in-band signal
- **Retry with backoff** on lock contention and stale working copy errors
- **Binary-safe output** (`Vec<u8>`) with convenient `.stdout_lossy()` for text
- **Repo detection** that walks parent directories and distinguishes git, jj, and colocated repos
- **Structured jj parsing** (optional) turns `jj log` and `jj bookmark list` output into typed Rust structs

## Usage

```toml
[dependencies]
vcs-runner = "0.7"
```

Git-only consumers can skip the jj parsing types and their serde dependencies:

```toml
[dependencies]
vcs-runner = { version = "0.7", default-features = false }
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

`run_jj` and `run_git` return `Result<RunOutput, RunError>`. The `RunError` enum distinguishes infrastructure failure (binary missing, fork failed) from non-zero exits (the command ran and reported failure via exit code):

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
- `err.is_non_zero_exit()` / `err.is_spawn_failure()` — check the variant
- `err.stderr()` — captured stderr on `NonZeroExit`, `None` on `Spawn`
- `err.exit_status()` — exit status on `NonZeroExit`, `None` on `Spawn`
- `err.program()` — the program name that failed

### Commands other than jj/git

```rust
use vcs_runner::{run_cmd, run_cmd_in, run_cmd_in_with_env, run_cmd_inherited};

// Captured output
let output = run_cmd("mise", &["env"])?;

// In a specific directory
let output = run_cmd_in(&repo_path, "make", &["build"])?;

// With environment variables (e.g., for GIT_INDEX_FILE)
let output = run_cmd_in_with_env(
    &repo_path, "git", &["add", "-N", "--", "file.rs"],
    &[("GIT_INDEX_FILE", "/tmp/index.tmp")],
)?;

// Inherited I/O (user sees output directly)
run_cmd_inherited("cargo", &["test"])?;
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

## Parsing jj output

Requires the `jj-parse` feature (on by default). Pre-built templates produce line-delimited JSON; parse functions handle malformed output gracefully.

```rust
use vcs_runner::{run_jj, BOOKMARK_TEMPLATE, LOG_TEMPLATE};
use vcs_runner::{parse_bookmark_output, parse_log_output};

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

// Skipped entries (malformed JSON from stale bookmarks, etc.)
for name in &result.skipped {
    eprintln!("skipped: {name}");
}
```

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
