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
- `jj-versions`: the whole test suite against jj 0.33.0, 0.36.0, 0.37.0, 0.38.0, 0.40.0 and 0.45.1 (see "Supported jj versions")

Release workflow publishes to crates.io on version-bump push to main.

`fuzz-replay.yml` replays every fuzz target's saved corpus on each push to `main` and each PR (the gate). `fuzz.yml` is a ~3-minute burst per target on each push to `main` and on `workflow_dispatch`; it saves the grown corpus and never gates. There is no scheduled fuzzing. See "Fuzzing".

## Supported jj versions

**jj 0.33 (2025-09) and later.** The floor is about a year of releases back from 0.45 (2026-09). It is below jjpr's own floor (0.36), and no other dependent states one. Nothing here refuses an older jj, but nothing is tested against one either.

The proof is real output, not the changelog. `src/jj_compat_tests.rs` has one test per captured version, reading `tests/fixtures/jj/<version>/`. That output comes from a scratch repository with file and directory renames, a trailing-space path and a non-ASCII path, local, pushed, moved and deleted bookmarks, a `feat@v2` name, a conflicted bookmark and a divergent change. CI's `jj-versions` job runs the whole test suite, including every test that shells out to jj, against 0.33.0, 0.36.0, 0.37.0, 0.38.0, 0.40.0 and 0.45.1.

To add a version, download the jj release binary and capture (the capture isolates itself from your jj config), then add a `version_tests!` line:

```sh
VCS_RUNNER_CAPTURE_JJ=/path/to/jj cargo test --lib capture_fixtures -- --ignored
```

What changed across the range, and what vcs-runner does about it:

| Surface | Changed in | Shape | Handling |
|---|---|---|---|
| `divergent()` revset | added in 0.38 | "Function `divergent` doesn't exist" before | `src/jj_version.rs` reads `jj --version` once; before 0.38 (or unknown) it uses `-r 'all()' -T 'if(divergent, …)'` |
| `jj diff --summary` renames/copies | since 0.21 (copy info in all diff formats) | `R prefix/{old => new}/suffix`, unchanged 0.33 to 0.45 | brace decoding; `R old -> new` still accepted, though no jj in range prints it |
| `jj bookmark list -T` | unchanged 0.33 to 0.45 | one line per ref: local, each tracked remote that points elsewhere, remote-only after a local delete; `<Error: No Commit available>` for conflicted | `BOOKMARK_TEMPLATE` emits `remote`; see `src/parse_bookmark.rs` |
| `RefSymbol` in templates | `.remote()` became `Option` in 0.30 | `stringify(name)` quotes names needing it (`"feat@v2"@origin`); `escape_json()` does not | `BOOKMARK_TEMPLATE` uses raw pairs; `LogEntry::remote_bookmarks` keeps jj's quoted symbol form |
| `LOG_TEMPLATE`, op-log template, `--at-operation`, `--ignore-working-copy`, `present()`, `latest()` | unchanged in range | | none needed |
| git empty tree in colocated repos | written from 0.38 | before, `git fsck` reports `missing tree 4b825dc…` | jj's behaviour, not ours; the colocation tests allow exactly that line |

Not supported: jj before 0.33 is untested. Before 0.30, `bookmark.remote()` was not an `Option`, and the templates' `if(remote, …)` has not been checked against that.

## Fuzzing

vcs-runner's whole job is parsing the stdout of jj and git, a format it does not version, holding paths and descriptions users typed. Every consumer inherits what these parsers get wrong. The method is the `rust-fuzzing` skill; this section is what is specific to this crate.

`fuzz/` is a standalone workspace built only by `cargo +nightly fuzz`. Crate-private parsers are reached through `src/fuzz_api.rs`, which exists only under `--cfg fuzzing` and so is not part of the published API.

| Target | Kind | Asserts |
|---|---|---|
| `vcs_output` | never-panics | every parser survives any bytes; each yields at most one record per line, and `parse_log_output` accounts for every non-blank line as an entry or a skipped line |
| `log_roundtrip` | roundtrip | fields rendered as `LOG_TEMPLATE` output (serde_json encoding, as jj's `escape_json()`) come back exactly; same for the `jj op log` template in `jj_operation_log` |
| `bookmark_roundtrip` | roundtrip | `BOOKMARK_TEMPLATE` output in jj's one-line-per-ref shape (local lines, remote lines for remotes that point elsewhere, conflicted and deleted `<Error: No Commit available>` lines) reads back exactly; remote status matches the answer the target built in, not the parser's rule |
| `diff_summary_roundtrip` | roundtrip | a change list rendered as `jj diff --summary` (brace renames under any factoring, and `old -> new`) and as `git diff --name-status` (with git's C quoting) parses back exactly |
| `detect_root` | invariant | on a real directory chain with fuzzed markers, `detect_vcs` of an absolute or relative path through `..`, `.` and missing directories returns exactly the nearest repository the path is really inside, or an error |

Vocabulary: `fuzz/dict/vcs_output.dict` holds every JSON key of `LOG_TEMPLATE` and `BOOKMARK_TEMPLATE` (`tests/fuzz_targets_wired.rs` fails when a template gains a key the dictionary lacks), plus the literals the parsers branch on. Seeds: `fuzz/corpus/vcs_output/seed-*` are test fixtures and real output captured from jj 0.33.0 and 0.45.1 (copies of `tests/fixtures/jj/`) and git; the structure-aware targets keep each crash they found as a `seed-*` plus a few grown inputs.

Frames the roundtrip targets hold, and so cannot see past:
- jj does not quote paths, so a rename whose path holds `{`, `}` or `>` is ambiguous and not generated; nor is a line break in a jj path.
- A bookmark name is used by one bookmark only, and a remote name is never empty, as in jj. The legacy `name@remote` rule (output without `remote`) is covered by unit tests only.
- Op-log descriptions hold no line break and no trailing `\r`.

Found on the first runs (2026-09-26), each fixed with a unit test beside the code: jj's `R src/{a => b}` renames were all dropped; git's quoted paths came back quoted; trailing whitespace was trimmed off paths; a stale bookmark's escaped name was mangled; `feat@v2@origin` counted as `feat`'s remote; `detect_vcs` walked lexical ancestors, crediting `sub` for `sub/../other` and stopping a relative path at the working directory. The per-version fixtures then found that `jj bookmark list` prints one line per ref, so remote refs came back as extra bookmarks with wrong statuses (every jj 0.33 to 0.45), and that `divergent()` does not exist before jj 0.38.

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
- `src/parse_jj.rs` — jj output parsers (log JSON, diff summary, remote list), gated behind `jj-parse` feature
- `src/parse_bookmark.rs` — `BOOKMARK_TEMPLATE` and its parser, gated behind `jj-parse` feature
- `src/jj_version.rs` — the installed jj's version, and the queries whose spelling depends on it
- `src/jj_compat_tests.rs` — per-jj-version fixture tests and their capture (test-only)
- `src/parse_git.rs` — git output parsers (diff name-status), gated behind `git-parse` feature
- `src/parse_op.rs` — line parsers for the op-log helpers in `runner.rs` (ungated, crate-private)
- `src/fuzz_api.rs` — fuzz-only entry points to crate-private parsers, compiled only under `cargo fuzz`
- `tests/file_length.rs` — the file-length ratchet: 400 production lines per file, `src/runner.rs` pinned; new code goes in a new module
- `src/types.rs` — shared types like `LogEntry`, `Bookmark`, `FileChange`

The current self-contained implementation will eventually move its generic subprocess primitives to depend on `procpilot`. Until that migration completes, vcs-runner ships its own `RunError`/`RunOutput`/etc.
