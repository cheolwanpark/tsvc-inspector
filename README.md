# tsvc-inspector

`tsvc-inspector` is a terminal UI for exploring TSVC benchmark kernels with a clang-driven analysis loop. It lets you browse benchmarks, select a function, inspect the pass timeline, review changed IR, and retry the same kernel under different compiler settings without leaving the TUI.

## What You Can Do

- Browse discovered TSVC benchmarks and read the extracted kernel-focused C source beside the list.
- Pick a specific loop/function before entering the detail page.
- Run analysis automatically on detail entry and inspect pass-by-pass IR changes.
- Toggle compiler settings from inside the app and compare how vectorization-related passes behave.
- Open side-by-side diffs, inspect pass info, and copy a detail snapshot for notes or sharing.

## Prerequisites

- Rust toolchain for `cargo run`.
- A working `clang` binary. The default is `clang`, or pass a path with `--clang`.
- A TSVC-containing `llvm-test-suite` checkout, or let the app auto-clone one if `--tsvc-root` does not exist.
- A terminal large enough for the detail page. The app expects at least `100x30`.

## Run

Show the CLI surface:

```bash
cargo run -- --help
```

Run with defaults:

```bash
cargo run --
```

Run with explicit paths and parallelism:

```bash
cargo run -- \
  --tsvc-root /path/to/llvm-test-suite \
  --clang /path/to/clang \
  --build-root . \
  --jobs 8
```

CLI options:

- `--tsvc-root <PATH>`: TSVC root. Default: `llvm-test-suite`
- `--clang <BIN>`: clang executable or path. Default: `clang`
- `--build-root <PATH>`: directory for build artifacts. Default: `.`
- `--jobs <N>`: jobs hint shown in build logs. Default: available parallelism

## TUI Workflow

### 1. Start On The Benchmark List

The opening page has two panes:

- `Benchmarks`: discovered TSVC cases
- `C Source (kernel-focused)`: extracted source for the current benchmark

Use this page to find the benchmark you want and read the relevant kernel code before analysis.

Keys:

- `Up` / `Down`: move the selected benchmark or scroll source, depending on focus
- `Left` / `Right`: switch focus between the benchmark list and source pane
- `Enter`: open the function picker
- `c`: open the global configuration modal
- `q`: quit

### 2. Pick A Function

`Enter` opens `Select Function`, which lists the benchmark's available loop/function targets as `loop_id (symbol)`.

Keys:

- `Up` / `Down`: move through functions
- `Enter`: open the detail page for the selected function
- `Esc`: close the modal

### 3. Inspect The Detail Page

Opening the detail page immediately starts analysis for the selected benchmark, function, and current compiler configuration.

The detail layout is split into:

- `Pass Timeline`: pass selector on the left
- `Inspector`: IR viewer on the right
- `Line Attributes`: fixed bottom bar for the selected IR line

Use this page to trace which passes ran, which ones changed IR, and how a pass rewrote the selected function.

Keys:

- `Left` / `Right`: switch focus between pass timeline and inspector
- `Up` / `Down`: move selected pass or move the IR cursor, depending on focus
- `Tab` / `Shift-Tab`: rotate the right pane between `IR Diff` and `IR`
- `Enter`: open pass info for the selected pass when the timeline is focused
- `r`: cycle the pass timeline filter between changed passes and all ran passes
- `c`: open the C source popup for the selected function
- `d`: open side-by-side diff for the selected pass
- `y`: copy a detail snapshot to the clipboard
- `Esc`: close an open modal, or return to the benchmark list

## Configuration Modal

Press `c` on the benchmark list to open the global configuration modal. Changes apply to future analyses immediately; there is no separate save step.

The modal includes grouped settings for optimization level, vectorization, loop transforms, targeting, and extra flags. The right side shows:

- `Option Guide`: what the selected option does and when it is useful
- `Flag Preview`: the effective analysis flags that will be used

Keys:

- `Up` / `Down`: move between config rows
- `Enter`: toggle/cycle the selected option, or start/finish editing a text field
- `Backspace`: delete while editing a text field
- text input: type extra C or LLVM flags into the active text field
- `Esc`: cancel text editing or close the modal

Suggested usage:

- Start with a lower optimization level such as `-O1` if you want an easier-to-read pass sequence.
- Disable vectorizers first, then re-enable them selectively to isolate effects.
- Use `Force Vec Width` and `Force Interleave` to test specific vectorization hypotheses.

## Behavior Notes

- If `--tsvc-root` does not point to a usable TSVC tree, the app can clone a sparse `llvm-test-suite` checkout into an app-managed temporary directory and continue from there.
- The app reports one of two function filtering modes at startup:
  `real-selective` when it can patch the managed fallback tree for function-scoped runs, or `output-filter` when it must analyze more broadly and filter results afterward.
- Build artifacts are written under the selected build root, primarily in `build-tsvc-native/analysis/<config_id>/...`.
- Analysis sessions are scoped by benchmark, selected function, and compiler configuration, so changing config creates a separate result set.

## Typical Session

1. Launch the app with `cargo run --`.
2. Choose a benchmark from the list and review the kernel-focused source pane.
3. Press `Enter`, select a function, and wait for analysis to populate the detail page.
4. Use `r`, `Tab`, `d`, and `c` to inspect how specific passes changed the function.
5. Return to the list with `Esc`, open the config modal with `c`, adjust flags, and run the same function again.
