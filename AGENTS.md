# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` wires CLI parsing, app startup, and the Ratatui event loop.
- `src/app.rs` owns UI state, page routing (`list -> compile-config -> detail`), per-benchmark sessions, 4-pane detail focus + scroll state, and job events; `src/ui.rs` renders views; `src/input.rs` maps keys to actions.
- `src/benchmark_manifest.rs` is the source of truth for benchmark names and run options.
- `src/syntax.rs` provides tree-sitter based syntax highlighting for C and LLVM IR with a bounded in-memory cache and graceful plain-text fallback.
- `src/discovery.rs`, `src/runner.rs`, `src/parser.rs`, and `src/bootstrap.rs` cover benchmark discovery (including kernel-focused source extraction), native clang runtime/analysis execution, analysis timeline parsing (fast trace + snapshots with `IrLine`/`source_line_map` generation), and TSVC root resolution.
- `src/model.rs` contains shared domain types (`IrLine`, `AnalysisStep`, `AnalysisStage`, etc.); `src/error.rs` defines common error/result aliases.
- Build outputs are generated in `target/` and config-scoped native folders like `build-tsvc-native/run/<config_id>/...` and `build-tsvc-native/analysis/<config_id>/...`; keep generated artifacts out of commits.

## TUI Navigation Model
- The app has three pages: `Benchmark List`, `Compile Config`, and `Benchmark Detail`.
- `Enter` on list opens an intermediate `Select Function` modal first.
- In `Select Function` modal: `Up`/`Down` moves selection, `Enter` confirms and opens `Compile Config`, `Esc` cancels.
- Benchmark list page has two focus panes: `Benchmarks` and `C Source (kernel-focused)`.
- On list page, `Tab`/`Shift-Tab` switches focus between `Benchmarks` and `C Source`.
- On list page, `Up`/`Down` acts on the focused pane:
  - `Benchmarks` focus: move selected benchmark.
  - `C Source` focus: scroll source text.
- List-page source text is derived from `tsc.c` and filtered `tsc.inc` sections selected by `#define TESTS ...` for the benchmark; common timing/harness lines are omitted for readability.

### Compile Config Page
- Config rows expose compiler knobs only: optimization level, vectorizer toggles, remark toggles, print-changed, debug info, and extra C/LLVM flags.
- `Up`/`Down` moves selected row.
- `Left`/`Right` changes/toggles selected row.
- `Enter` toggles config rows or enters/exits text edit mode for extra flag rows.
- `d` persists config for the selected benchmark/function and opens detail page.
- `Esc` cancels text editing (if active) or returns to benchmark list page.
- Config/session scope is `benchmark + selected function + config_id`.

### Benchmark Detail Page (2x2 Grid Layout)
- The detail page uses a 2x2 grid layout:
  - **Top row (30%)**: Stage list (25% width) | Pass list (75% width)
  - **Bottom row (70%)**: C Source (35% width) | IR View (65% width)
- Four focus panes: `StageList`, `PassList`, `SourceView`, `IrView`.
- `Tab`/`Shift-Tab` cycles through all 4 panes (wrapping around).
- `Up`/`Down` acts on the focused pane:
  - `StageList`: move selected analysis stage.
  - `PassList`: move selected pass within stage.
  - `SourceView`: scroll C source text.
  - `IrView`: scroll full-function IR (interleaved diff view).
- `Enter`: StageList -> PassList, PassList -> IrView.
- `Esc`: IrView -> PassList -> StageList -> back to list page.
- `a` runs analysis, `r` runs build+run, `y` copies a detail snapshot to clipboard, `c` clears session.
- Detail sessions are scoped per `benchmark + selected function + config_id`.
- Function selection is required before entering detail.
- Minimum terminal size: 100x30.

### IR View
- IR View shows full function IR with interleaved diff highlighting:
  - Inserted lines: `+ ` prefix with syntax-highlighted text over a dark green background (`IR_INSERT_BG`).
  - Deleted lines: `- ` prefix with syntax-highlighted text over a dark red background (`IR_DELETE_BG`).
  - Unchanged lines: `  ` prefix with syntax-highlighted text over a dark code background (`CODE_BG`).
- IR data is stored as `Vec<IrLine>` (with `similar::ChangeTag`) per `AnalysisStep`.
- LLVM IR syntax highlighting is provided by `tree-sitter-highlight` + `tree-sitter-llvm`, and diff backgrounds remain visible via style patching.
- Clipboard snapshot (`y`) includes the selected stage/pass metadata, linked remarks, selected-function C source, and full IR diff for the selected pass.

### C Source Panel (Detail)
- Shows only the selected target function's C source with line numbers.
- Function text is extracted from the benchmark's kernel-focused source; if extraction fails, the panel shows an explicit unavailable message.
- C syntax highlighting is provided by `tree-sitter-highlight` + `tree-sitter-c` in both list-page and detail-page source panels.
- `!dbg` metadata in IR is parsed to build `source_line_map` for potential C<->IR matching.
- Note: `!dbg` line numbers are absolute file positions; the source panel shows a function-only excerpt with different numbering, so C<->IR highlighting is not yet functional (degrades gracefully with no highlights).

### Verdict System
- Header shows vectorization verdict based on optimization remarks.
- Verdict fallback: when remarks are empty but `loopvectorize`/`slpvectorizer` passes made IR changes, shows "~ LIKELY VECTORIZED" (Cyan).
- Analysis compile flags include `-g` to enable `!dbg` metadata in IR output.

## Function-Selective Run Notes
- The app supports function-selective runs with two modes:
  - `real-selective`: available when using app-managed fallback TSVC root and patching `tsc.inc` succeeds.
  - `output-filter`: used for external TSVC roots or when fallback patching fails.
- In both modes, runtime output is filtered by selected function (loop rows, remarks).
- Build path uses a native clang compile/link pipeline (no CMake configure step), incremental target cleanup, and parallelism hint (`jobs`) for consistency with app settings.

## Analysis Workflow Notes
- Optimization-path exploration is primary:
  - Fast tier: parse `-mllvm -print-changed` trace and build function-scoped changed-only timeline with `IrLine` generation and `!dbg` metadata parsing.
  - When `print-changed` is enabled, analysis compile adds `-mllvm -filter-print-funcs=<selected_symbol>` to keep traces focused and bounded.
- Runtime (`r`) is secondary and intentionally lightweight; it does not regenerate full IR timelines.

## Build, Test, and Development Commands
- `cargo check`: fast compile validation during development.
- `cargo run -- --tsvc-root /path/to/llvm-test-suite --build-root . --clang /path/to/clang --jobs 8`: start the TUI with explicit toolchain/build settings.
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
- Use descriptive test names such as `parses_tsvc_rows` and `config_build_dir_contains_config_id`.
- Cover both happy paths and failure/edge handling (missing files, malformed output, absent binaries, missing function IR in snapshots).
- Keep tests deterministic; avoid network- or environment-dependent behavior.

## Commit & Pull Request Guidelines
- Match existing history style: short, imperative subject lines (example: `Add TSVC TUI app and ignore generated artifacts`).
- Keep commits single-purpose and explain non-obvious decisions in the commit body.
- PRs should include: summary of behavior changes, verification commands run (for example `cargo test` and `cargo clippy`), linked issues, and a screenshot or terminal capture when UI behavior changes.
