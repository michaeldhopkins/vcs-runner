# vcs-runner

VCS-specific helpers on top of generic subprocess execution: jj/git wrappers, repo detection (`detect_vcs`), output parsers (jj log/bookmark JSON, git diff name-status), and merge-base helpers. Library crate consumed by branchdiff, jjpr, specdiff, workon, and similar VCS tooling.

## Pre-commit checklist

Before every commit, verify:
1. [ ] `cargo clippy --all-features --all-targets -- -D warnings` passes
2. [ ] `cargo clippy --no-default-features --all-targets -- -D warnings` passes
3. [ ] `cargo test --all-features` passes
4. [ ] `cargo test --no-default-features` passes
5. [ ] `cargo test --doc --all-features` passes
6. [ ] Version bumped in `Cargo.toml` (patch for fixes/docs, minor for features, `0.x` breaking bumps minor)
7. [ ] `cargo check` run after version bump (updates `Cargo.lock`)
8. [ ] If the release will generate user-visible changes, `git cliff --output CHANGELOG.md`

Never use `#[allow(...)]` to suppress warnings — fix the underlying issue.

## Antipatterns (not caught by lints)

**Stringly-typed error handling.** Don't use `.contains()` on stderr strings to branch logic. Use the typed `RunError` variants. Match on structure.

**Panics in error handlers.** `panic!()` inside closures is user-hostile. Library code returns `Result`. `.expect("reason")` is acceptable only for invariants that are truly impossible to violate at runtime.

**Unnecessary cloning.** Watch for `.clone().or(.clone())`. Watch for functions taking `&T` that internally clone. Cloning `Arc`s and `PathBuf`s for thread/retry use is correct.

**Code duplication.** If the same logic appears 3+ times, extract a helper. Lints don't catch semantic duplication.

**Local fixes that ignore root cause.** Adding `.clone()` to satisfy the borrow checker instead of restructuring. Wrapping errors in strings instead of adding enum variants.

**Feature-gate leaks.** Items gated behind a feature should not be visible in docs when the feature is off (`#[cfg_attr(docsrs, doc(cfg(feature = "...")))]`).

## Testing requirements

**Every behavioral change requires tests.** This is non-negotiable.

- New functions: unit tests covering happy path + at least one edge case
- Bug fixes: write a failing test first, then fix
- Refactors: existing tests must pass; add tests if coverage gaps surface

Tests that need real `jj` or `git` binaries should be guarded with `if !binary_available("jj") { return; }` so they're skipped in environments where the binary isn't installed (don't fail). CI runs in environments where both binaries are available.

## Semver (0.x conventions)

**PATCH (0.x.Y → 0.x.Y+1):** bug fixes, docs, internal refactor, dep updates, test additions, non-breaking doc-comment changes.

**MINOR (0.X.y → 0.X+1.0):** new public APIs, new features, or **any breaking change** (standard semver 0.x convention — minor bumps are breaking during 0.x).

For breaking releases, document migration steps in the commit message and release notes. Create a `MIGRATION.md` if cumulative breakage gets complex enough to need a dedicated guide.

## CI expectations

CI runs on push/PR:
- `cargo check --locked` (default and `--no-default-features`)
- `cargo test --locked` (default and `--no-default-features`)
- `cargo clippy --locked --all-targets -- -D warnings` (both feature configs)
- `cargo doc --no-deps` with `RUSTDOCFLAGS="-D warnings"` (catches broken doc links)
- `cargo deny check licenses`

Release workflow publishes to crates.io on version-bump push to main.

`fuzz-replay.yml` replays every fuzz target's saved corpus on each push to `main` and each PR (the gate). `fuzz.yml` is a ~3-minute burst per target on each push to `main` and on `workflow_dispatch`; it saves the grown corpus and never gates. There is no scheduled fuzzing. See "Fuzzing".

## Fuzzing

vcs-runner's whole job is parsing the stdout of jj and git, a format it does not version, holding paths and descriptions users typed. Every consumer inherits what these parsers get wrong. The method is the `rust-fuzzing` skill; this section is what is specific to this crate.

`fuzz/` is a standalone workspace built only by `cargo +nightly fuzz`. Crate-private parsers are reached through `src/fuzz_api.rs`, which exists only under `--cfg fuzzing` and so is not part of the published API.

| Target | Kind | Asserts |
|---|---|---|
| `vcs_output` | never-panics | every parser survives any bytes; each yields at most one record per line, and `parse_log_output` accounts for every non-blank line as an entry or a skipped line |
| `log_roundtrip` | roundtrip | fields rendered as `LOG_TEMPLATE` output (serde_json encoding, as jj's `escape_json()`) come back exactly; same for the `jj op log` template in `jj_operation_log` |
| `bookmark_roundtrip` | roundtrip | `BOOKMARK_TEMPLATE` output, including stale bookmarks' `<Error: No Commit available>` lines, reads back exactly; remote status matches the answer the target built in, not the parser's rule |
| `diff_summary_roundtrip` | roundtrip | a change list rendered as `jj diff --summary` (brace renames under any factoring, and `old -> new`) and as `git diff --name-status` (with git's C quoting) parses back exactly |
| `detect_root` | invariant | on a real directory chain with fuzzed markers, `detect_vcs` of an absolute or relative path through `..`, `.` and missing directories returns exactly the nearest repository the path is really inside, or an error |

Vocabulary: `fuzz/dict/vcs_output.dict` holds every JSON key of `LOG_TEMPLATE` and `BOOKMARK_TEMPLATE` (`tests/fuzz_targets_wired.rs` fails when a template gains a key the dictionary lacks), plus the literals the parsers branch on. Seeds: `fuzz/corpus/vcs_output/seed-*` are test fixtures and real output captured from jj 0.45 and git; the structure-aware targets keep each crash they found as a `seed-*` plus a few grown inputs.

Frames the roundtrip targets hold, and so cannot see past:
- jj does not quote paths, so a rename whose path holds `{`, `}` or `>` is ambiguous and not generated; nor is a line break in a jj path.
- Remote names are generated without `@`; the owner of `name@remote` is read as everything before the last `@`.
- Op-log descriptions hold no line break and no trailing `\r`.

Found on the first runs (2026-09-26), each fixed with a unit test beside the code: jj's `R src/{a => b}` renames were all dropped; git's quoted paths came back quoted; trailing whitespace was trimmed off paths; a stale bookmark's escaped name was mangled; `feat@v2@origin` counted as `feat`'s remote; `detect_vcs` walked lexical ancestors, crediting `sub` for `sub/../other` and stopping a relative path at the working directory.

Not fuzzed: the `run_*` wrappers (argument passing to a subprocess; procpilot owns that), `read_working_file*` (a filesystem read of a caller-supplied path; note that `rel_path` is joined unchecked, so `../x` or an absolute path reads outside the repo, which is the caller's contract to uphold), and `parse_remote_list`'s semantics beyond not panicking (a space-split with nothing to get wrong).

Run locally from the repo root:

```sh
cargo +nightly fuzz build
fuzz/burst.sh fuzz/target/aarch64-apple-darwin/release/<target> <target> 60   # mutate, then merge finds
fuzz/target/aarch64-apple-darwin/release/<target> -runs=0 fuzz/corpus/<target> # replay only
```

Run one target at a time: parallel sanitizer builds manufacture `slow-unit` artifacts from contention. `detect_root` changes the working directory per input and restores it before asserting, since libFuzzer writes artifacts relative to it.

## Architecture notes

- `src/lib.rs` re-exports the public API; nothing lives directly here except crate-level docs and module declarations
- `src/runner.rs` — `RunError`, `RunOutput`, `run_jj`, `run_git`, retry/timeout helpers, `binary_available`, merge-base helpers
- `src/error.rs` — `RunError` definition (separate from runner for clarity)
- `src/detect.rs` — `detect_vcs`, `VcsBackend` (filesystem-based detection that walks ancestor dirs)
- `src/parse_jj.rs` — jj output parsers (log JSON, bookmark JSON, diff summary), gated behind `jj-parse` feature
- `src/parse_git.rs` — git output parsers (diff name-status), gated behind `git-parse` feature
- `src/parse_op.rs` — line parsers for the op-log helpers in `runner.rs` (ungated, crate-private)
- `src/fuzz_api.rs` — fuzz-only entry points to crate-private parsers, compiled only under `cargo fuzz`
- `tests/file_length.rs` — the file-length ratchet: 400 production lines per file, `src/runner.rs` pinned; new code goes in a new module
- `src/types.rs` — shared types like `LogEntry`, `Bookmark`, `FileChange`

The current self-contained implementation will eventually move its generic subprocess primitives to depend on `procpilot`. Until that migration completes, vcs-runner ships its own `RunError`/`RunOutput`/etc.
