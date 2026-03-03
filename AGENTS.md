# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` wires CLI parsing, app startup, and the Ratatui event loop.
- `src/app.rs` owns UI state, page routing (list/detail), per-benchmark sessions, list/detail focus + scroll state, and job events; `src/ui.rs` renders views; `src/input.rs` maps keys to actions.
- `src/discovery.rs`, `src/runner.rs`, `src/parser.rs`, and `src/bootstrap.rs` cover benchmark discovery (including kernel-focused source extraction), runtime/analysis execution, analysis timeline parsing (fast trace + deep snapshots), and TSVC root resolution.
- `src/model.rs` contains shared domain types; `src/error.rs` defines common error/result aliases.
- Build outputs are generated in `target/` and profile-specific folders such as `build-tsvc-o3-remarks-run/` and `build-tsvc-o3-remarks-analysis/`; keep generated artifacts out of commits.

## TUI Navigation Model
- The app has two pages: `Benchmark List` and `Benchmark Detail`.
- `Enter` on list opens an intermediate `Select Function` modal first.
- In `Select Function` modal: `Up`/`Down` moves selection, `Enter` confirms and opens detail, `Esc` cancels.
- `Esc` on detail returns to the list page.
- Runtime keys (`b`, `r`, `a`) and analysis keys (`x`, `X`) are active on the benchmark detail page only.
- `x` runs fast analysis (trace-based function timeline).
- `X` runs deep analysis (windowed `opt` bisect snapshots around selected step).
- Benchmark list page has two focus panes: `Benchmarks` and `C Source (kernel-focused)`.
- On list page, `Left`/`Right` switches focus between `Benchmarks` and `C Source`.
- On list page, `Up`/`Down` acts on the focused pane:
  - `Benchmarks` focus: move selected benchmark.
  - `C Source` focus: scroll source text.
- List-page source text is derived from `tsc.c` and filtered `tsc.inc` sections selected by `#define TESTS ...` for the benchmark; common timing/harness lines are omitted for readability.
- Benchmark detail page has two focus panes: `IR Steps` and `IR Diff`.
- On detail page, `Left`/`Right` switches focus between `IR Steps` and `IR Diff`.
- On detail page, `Up`/`Down` acts on the focused pane:
  - `IR Steps` focus: move selected optimization step.
  - `IR Diff` focus: scroll IR diff text.
- `o` toggles remarks/analysis overlay on top of the IR diff.
- Detail sessions are scoped per `benchmark + selected function`.
- Function selection is required before entering detail.
- IR-step rows show metadata such as raw pass index, stage, pass occurrence, source (`trace`/`deep`), and matched-remark counts.

## Function-Selective Run Notes
- The app supports function-selective runs with two modes:
  - `real-selective`: available when using app-managed fallback TSVC root and patching `tsc.inc` succeeds.
  - `output-filter`: used for external TSVC roots or when fallback patching fails.
- In both modes, runtime output is filtered by selected function (loop rows, remarks).
- Build path is incremental and parallelized (`-j`), without `--clean-first`.

## Analysis Workflow Notes
- Optimization-path exploration is primary:
  - Fast tier: parse `-mllvm -print-changed` trace and build function-scoped changed-only timeline.
  - Deep tier: compile `tsc.c` to bitcode and reconstruct a local pass window via `opt -opt-bisect-limit`.
- Runtime (`b`/`r`/`a`) is secondary and intentionally lightweight; it does not regenerate full IR timelines.
- Running runtime jobs after analysis marks analysis state as `stale`; users should rerun `x` or `X` for refreshed IR steps.

## Build, Test, and Development Commands
- `cargo check`: fast compile validation during development.
- `cargo run -- --tsvc-root /path/to/llvm-test-suite --build-root . --opt /path/to/opt --analysis-window 80`: start the TUI with explicit analysis tooling/options.
- `cargo run`: start with defaults (`--tsvc-root llvm-test-suite`), with fallback clone logic in `bootstrap.rs`.
- `cargo test`: run unit tests embedded in module `#[cfg(test)]` blocks.
- `cargo fmt --check`: verify formatting.
- `cargo clippy --all-targets --all-features -- -D warnings`: enforce lint-clean changes before review.

## Coding Style & Naming Conventions
- Use Rust 2024 idioms and `rustfmt` defaults (4-space indentation, trailing commas where helpful).
- Follow standard Rust naming: `snake_case` for functions/modules/files, `PascalCase` for types/enums, `UPPER_SNAKE_CASE` for constants.
- Prefer small, focused functions and explicit error context via `anyhow::Context`.
- Keep module boundaries clear: UI code in `ui.rs`, parsing in `parser.rs`, process execution in `runner.rs`.

## Testing Guidelines
- Place tests next to the code they validate (`#[cfg(test)] mod tests` in each module).
- Use descriptive test names such as `parses_tsvc_rows` and `profile_build_dir_names_are_stable`.
- Cover both happy paths and failure/edge handling (missing files, malformed output, absent binaries, missing function IR in snapshots).
- Keep tests deterministic; avoid network- or environment-dependent behavior.

## Commit & Pull Request Guidelines
- Match existing history style: short, imperative subject lines (example: `Add TSVC TUI app and ignore generated artifacts`).
- Keep commits single-purpose and explain non-obvious decisions in the commit body.
- PRs should include: summary of behavior changes, verification commands run (for example `cargo test` and `cargo clippy`), linked issues, and a screenshot or terminal capture when UI behavior changes.
