# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` wires CLI parsing, app startup, and the Ratatui event loop.
- `src/app.rs` owns UI state, page routing (list/detail), per-benchmark sessions, focus/scroll state for the detail page, and job events; `src/ui.rs` renders views; `src/input.rs` maps keys to actions.
- `src/discovery.rs`, `src/runner.rs`, `src/parser.rs`, and `src/bootstrap.rs` cover benchmark discovery, build/run execution, output parsing (remarks + IR diff step parsing), and TSVC root resolution.
- `src/model.rs` contains shared domain types; `src/error.rs` defines common error/result aliases.
- Build outputs are generated in `target/` and profile-specific folders such as `build-tsvc-o3-remarks/`; keep generated artifacts out of commits.

## TUI Navigation Model
- The app has two pages: `Benchmark List` and `Benchmark Detail`.
- `Enter` opens the selected benchmark detail page; `Esc` returns to the list page.
- Build/run keys (`b`, `r`, `a`) are active on the benchmark detail page only.
- Benchmark list page uses `Up`/`Down` for benchmark selection.
- Benchmark detail page has two focus panes: `IR Steps` and `IR Diff`.
- On detail page, `Left`/`Right` switches focus between `IR Steps` and `IR Diff`.
- On detail page, `Up`/`Down` acts on the focused pane:
  - `IR Steps` focus: move selected optimization step.
  - `IR Diff` focus: scroll IR diff text.
- `o` toggles remarks/analysis overlay on top of the IR diff.
- Sessions are scoped per benchmark, with latest-run data shown for the currently selected benchmark.

## Build, Test, and Development Commands
- `cargo check`: fast compile validation during development.
- `cargo run -- --tsvc-root /path/to/llvm-test-suite --build-root .`: start the TUI with an explicit TSVC root.
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
- Cover both happy paths and failure/edge handling (missing files, malformed output, absent binaries).
- Keep tests deterministic; avoid network- or environment-dependent behavior.

## Commit & Pull Request Guidelines
- Match existing history style: short, imperative subject lines (example: `Add TSVC TUI app and ignore generated artifacts`).
- Keep commits single-purpose and explain non-obvious decisions in the commit body.
- PRs should include: summary of behavior changes, verification commands run (for example `cargo test` and `cargo clippy`), linked issues, and a screenshot or terminal capture when UI behavior changes.
