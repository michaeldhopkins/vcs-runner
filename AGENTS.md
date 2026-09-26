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

## Architecture notes

- `src/lib.rs` re-exports the public API; nothing lives directly here except crate-level docs and module declarations
- `src/runner.rs` — `RunError`, `RunOutput`, `run_jj`, `run_git`, retry/timeout helpers, `binary_available`, merge-base helpers
- `src/error.rs` — `RunError` definition (separate from runner for clarity)
- `src/detect.rs` — `detect_vcs`, `VcsBackend` (filesystem-based detection that walks ancestor dirs)
- `src/parse_jj.rs` — jj output parsers (log JSON, bookmark JSON, diff summary), gated behind `jj-parse` feature
- `src/parse_git.rs` — git output parsers (diff name-status), gated behind `git-parse` feature
- `src/types.rs` — shared types like `LogEntry`, `Bookmark`, `FileChange`

The current self-contained implementation will eventually move its generic subprocess primitives to depend on `procpilot`. Until that migration completes, vcs-runner ships its own `RunError`/`RunOutput`/etc.
