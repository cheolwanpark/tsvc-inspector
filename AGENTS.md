# Global Instructions

- Always activate the `karpathy-guidelines` skill at the start of every task.
- When reading web documentation, use the `agent-browser` skill.
- Always check commit history before writing commit messages.

---

# Repository Guidelines

## Architecture Overview

This repository uses a **3-level architecture** with a small shared core:

1. `data`: external IO + raw data acquisition/parsing
2. `transform`: composition/enrichment of raw data into app-ready structures
3. `display`: Ratatui/TUI state, input, rendering, and UX
4. `core`: shared domain types and error alias only

The application is structured as a `lib + bin` crate:
- `src/lib.rs`: module graph (`core`, `data`, `transform`, `display`)
- `src/main.rs`: thin CLI entrypoint only

## Project Structure

- `src/core/model.rs`: canonical domain types (`BenchmarkItem`, `CompilerConfig`, `AnalysisStep`, `IrLine`, etc.)
- `src/core/error.rs`: common `AppResult` alias

- `src/data/repo.rs`: TSVC root resolution, fallback clone behavior, function-run mode configuration
- `src/data/runner.rs`: native clang build/run/analysis command wrappers and artifact discovery
- `src/data/parser.rs`: raw text/yaml parsing (`tsvc stdout`, `remarks`, `IR snapshots`, `dbg maps`)
- `src/data/discovery.rs`: raw benchmark discovery and source loading
- `src/data/manifest.rs`: benchmark manifest and run options
- `src/data/tsvc_patch.rs`: fallback root patching for real-selective function filtering

- `src/transform/catalog.rs`: build benchmark catalog from raw discovery output
- `src/transform/source.rs`: kernel-focused source shaping + function extraction
- `src/transform/analysis.rs`: IR diff timeline + source-annotation transform
- `src/transform/filtering.rs`: selected-function filtering helpers
- `src/transform/session.rs`: verdict helpers + detail snapshot payload generation

- `src/display/runtime.rs`: app startup/event loop/job orchestration wiring
- `src/display/app.rs`: UI state machine, navigation, sessions, focus/scroll behavior
- `src/display/ui.rs`: Ratatui rendering
- `src/display/input.rs`: key mapping
- `src/display/syntax.rs`: tree-sitter syntax highlighting + cache
- `src/display/clipboard.rs`: clipboard integration

Generated artifacts are under `target/` and `build-tsvc-native/...`; do not commit them.

## Layer Boundaries (Important)

- `display` may depend on `transform`, `data`, and `core`.
- `transform` may depend on `data` and `core`, but **never** on Ratatui/UI concerns.
- `data` may depend on `core`, but **never** on TUI/rendering state.
- `core` depends on no app-specific layers.

If logic is about:
- command/process/file/network/parse primitive output -> `data`
- combining/interpreting multiple raw sources into semantic view data -> `transform`
- focus, key handling, layout, widget styling, scroll, page routing -> `display`

## TUI Behavior Contract

### Pages and Flow
- Pages: `Benchmark List` -> `Compile Config` -> `Benchmark Detail`
- `Enter` on list opens `Select Function` modal first
- Session scope: `benchmark + selected function + config_id`

### List Page
- Two panes: `Benchmarks`, `C Source (kernel-focused)`
- `Tab` / `Shift-Tab`: switch focus pane
- `Up`/`Down`:
  - Benchmarks focus -> move selection
  - Source focus -> scroll source

### Compile Config Page
- Rows grouped into 5 sections:
  - Optimization
  - Vectorization
  - Loop Transforms
  - Target
  - Advanced
- `Up`/`Down`: move row
- `Left`/`Right`: adjust/toggle value
- `Enter`: toggle or enter text-edit mode
- `d`: persist config + open detail page
- `Esc`: cancel text edit or go back to list

### Detail Page (2x2)
- Top: Stage list (25%) | Pass list (75%)
- Bottom: C source (35%) | IR view (65%)
- Focus panes: `StageList`, `PassList`, `SourceView`, `IrView`
- `Tab` / `Shift-Tab`: cycle all 4 panes
- `Up`/`Down`: move in focused pane
- `Enter`: Stage -> Pass, Pass -> IR
- `Esc`: IR -> Pass -> Stage -> list
- Actions:
  - `a`: run analysis
  - `r`: run build+run
  - `y`: copy detail snapshot
  - `c`: clear session

Minimum terminal size: `100x30`.

## Analysis and IR Notes

- Analysis path is fast trace based (`-mllvm -print-changed`) and function-scoped.
- IR view uses interleaved diff lines:
  - `+ ` inserted
  - `- ` deleted
  - `  ` unchanged
- Source annotations (`;; ...`) are injected with diff-tag-consistent backgrounds.
- `#dbg_*` intrinsics are removed from displayed IR.
- Trailing metadata (`!dbg`, `!tbaa`, `!llvm.loop`, etc.) is stripped in transformed IR output.

## Function-Selective Run Modes

- `real-selective`: app-managed fallback root where patch succeeds
- `output-filter`: external root or patch failure

Both modes must preserve selected-function scoped output in UI/session behavior.

## Development Commands

- `cargo check`
- `cargo run -- --tsvc-root /path/to/llvm-test-suite --build-root . --clang /path/to/clang --jobs 8`
- `cargo run`
- `cargo test`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Coding Style

- Rust 2024 + `rustfmt` defaults
- Naming:
  - `snake_case` functions/modules/files
  - `PascalCase` types/enums
  - `UPPER_SNAKE_CASE` constants
- Prefer small, explicit functions with actionable error context via `anyhow::Context`.
- Avoid cross-layer leakage; extract shared semantics into `transform` or `core` instead of duplicating in `display`.

## Testing Guidelines

- Keep tests adjacent to implementation (`#[cfg(test)] mod tests`).
- Cover:
  - happy path
  - malformed/missing file and parse failures
  - function-scoped IR and remark edge cases
  - session scope correctness by benchmark/function/config
- Keep tests deterministic; avoid external network dependencies.

## Commit and PR Guidelines

- Follow existing style in history: concise, imperative subjects (commonly with prefixes like `feat:`, `fix:`, `refactor:`).
- Keep commits single-purpose when possible; explain non-obvious decisions in body.
- Before final review, ensure:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`

For UI-impacting changes, include terminal screenshots or captures in PR notes.
