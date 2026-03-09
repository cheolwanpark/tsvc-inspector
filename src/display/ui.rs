use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use similar::ChangeTag;

use crate::core::model::{
    AnalysisState, AnalysisStep, AppPage, PassTraceEntry, PassTraceStatus, RemarkEntry, RemarkKind,
    RunSession,
};
use crate::display::app::{AppState, CodeViewMode, ConfigRow};
use crate::display::syntax::{self, StyledChunk, SyntaxLang};
use crate::transform::session::has_vectorizer_ir_changes;

const CODE_BG: Color = Color::Rgb(14, 20, 28);
const CODE_TEXT_FG: Color = Color::Gray;
const SOURCE_LINE_HIGHLIGHT_BG: Color = Color::Rgb(44, 52, 64);
const IR_INSERT_BG: Color = Color::Rgb(19, 70, 35);
const IR_DELETE_BG: Color = Color::Rgb(90, 28, 28);
const IR_SELECTED_BG: Color = Color::Rgb(44, 58, 82);
const IR_INSERT_SELECTED_BG: Color = Color::Rgb(28, 92, 48);
const IR_DELETE_SELECTED_BG: Color = Color::Rgb(118, 42, 42);
const SOURCE_ANNOTATION_FG: Color = Color::Rgb(200, 160, 80);
const DETAIL_ATTR_BAR_HEIGHT: u16 = 7;

struct SideBySideDiffRow<'a> {
    left: Option<&'a crate::core::model::IrLine>,
    right: Option<&'a crate::core::model::IrLine>,
}

pub fn render(frame: &mut Frame, app: &mut AppState) {
    match app.page {
        AppPage::BenchmarkList => render_benchmark_list_page(frame, app),
        AppPage::BenchmarkDetail => render_benchmark_detail_page(frame, app),
    }
}

fn render_benchmark_list_page(frame: &mut Frame, app: &AppState) {
    let vertical = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ]);
    let [header_area, main_area, footer_area] = vertical.areas(frame.area());
    render_list_header(frame, app, header_area);

    let horizontal = Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]);
    let [list_area, source_area] = horizontal.areas(main_area);
    render_benchmarks(frame, app, list_area);
    render_benchmark_source_code(frame, app, source_area);
    render_list_footer(frame, app, footer_area);
    if app.is_config_modal_open() {
        render_config_modal(frame, app);
    }
    if app.is_function_modal_open() {
        render_function_select_modal(frame, app);
    }
}

fn render_config_modal(frame: &mut Frame, app: &AppState) {
    let area = centered_rect(frame.area(), 78, 78);
    frame.render_widget(Clear, area);

    let vertical = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]);
    let [main_area, footer_area] = vertical.areas(area);

    let cols = Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]);
    let [left, right] = cols.areas(main_area);
    let rows = app.config_rows();
    let mut items: Vec<ListItem> = Vec::new();
    let mut display_to_data: Vec<Option<usize>> = Vec::new();
    let mut prev_group: Option<&str> = None;

    for (i, row) in rows.iter().enumerate() {
        let group = row.group();
        if prev_group != Some(group) {
            let header_text = format!("  --- {} ---", group);
            items.push(
                ListItem::new(header_text).style(
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
            );
            display_to_data.push(None);
            prev_group = Some(group);
        }
        let value = app.config_row_value_text(*row);
        let row_suffix = if *row == app.config_selected_row_kind() && app.is_config_text_editing() {
            " [editing]"
        } else {
            ""
        };
        items.push(ListItem::new(format!(
            "  {:<18} : {}{}",
            row.title(),
            value,
            row_suffix
        )));
        display_to_data.push(Some(i));
    }

    let display_index = display_to_data
        .iter()
        .position(|entry| *entry == Some(app.config_selected_row))
        .unwrap_or(0);

    let list = List::new(items)
        .block(Block::bordered().title("Configuration"))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    let mut state = ListState::default();
    state.select(Some(display_index));
    frame.render_stateful_widget(list, left, &mut state);

    let right_rows = Layout::vertical([Constraint::Percentage(54), Constraint::Percentage(46)]);
    let [guide_area, preview_area] = right_rows.areas(right);

    let selected_row = app.config_selected_row_kind();
    let guide = Paragraph::new(Text::from(config_help_lines(app, selected_row)))
        .block(Block::bordered().title("Option Guide"))
        .wrap(Wrap { trim: false });
    frame.render_widget(guide, guide_area);

    let preview_text = Text::from(vec![
        Line::from("Analysis C Flags"),
        Line::from(app.config_analysis_flags_preview()),
    ]);
    let preview = Paragraph::new(preview_text)
        .block(Block::bordered().title("Flag Preview"))
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, preview_area);

    let hints = if app.is_config_text_editing() {
        "type text · backspace delete · enter finish · esc cancel edit"
    } else {
        "↑↓ row · enter toggle/edit · esc close"
    };
    let footer = Paragraph::new(Text::from(vec![
        Line::from("Config Modal"),
        Line::from(hints),
        Line::from(format!("Status: {}", app.status_message)),
    ]))
    .block(Block::bordered().title("Modal"));
    frame.render_widget(footer, footer_area);
}

fn config_help_lines(app: &AppState, row: ConfigRow) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(format!("Selected: {}", row.title())));
    lines.push(Line::from(""));

    let body = match row {
        ConfigRow::OptLevel => vec![
            "What: Chooses optimization aggressiveness (-O0..-Oz).",
            "Why: Lower levels show simpler pass effects; higher levels show full pipeline behavior.",
            "Tip: Start with -O1 to learn pass order, then raise to -O3.",
        ],
        ConfigRow::FastMath => vec![
            "What: Enables -ffast-math (FP reassociation, no NaN/Inf guards).",
            "Why: FP reassociation enables reduction vectorization for sums/products.",
            "Tip: Try on for reduction loops that fail to vectorize with strict FP.",
        ],
        ConfigRow::NoInlining => vec![
            "What: Disables standard inlining with -fno-inline.",
            "Why: Keeps call boundaries visible so pass effects are easier to inspect per function.",
            "Tip: Use this when inlining hides the kernel shape before vectorization analysis.",
        ],
        ConfigRow::LoopVectorize => vec![
            "What: Enables/disables loop vectorization (-fno-vectorize when off).",
            "Why: Isolates vectorizer impact from other loop optimizations.",
            "Tip: Compare on/off with same -O level to inspect changed passes.",
        ],
        ConfigRow::SlpVectorize => vec![
            "What: Enables/disables SLP vectorization (-fno-slp-vectorize when off).",
            "Why: Separates basic-block vectorization effects from loop vectorization.",
            "Tip: Disable this when focusing only on loop-vectorize remarks.",
        ],
        ConfigRow::ForceVecWidth => vec![
            "What: Overrides the vectorizer's VF choice (-mllvm -force-vector-width=N).",
            "Why: Tests specific vector factors regardless of cost model decisions.",
            "Tip: Set to 4 or 8 to force vectorization even when cost model says no.",
        ],
        ConfigRow::ForceInterleave => vec![
            "What: Overrides interleaving factor (-mllvm -force-vector-interleave=N).",
            "Why: Isolates vectorization from interleaving in the IR timeline.",
            "Tip: Set to 1 to see pure vectorization without unroll-and-jam.",
        ],
        ConfigRow::UnrollLoops => vec![
            "What: Enables/disables loop unrolling (-fno-unroll-loops when off).",
            "Why: Separates unrolling from vectorization in the IR timeline.",
            "Tip: Disable to see cleaner vectorized IR without unrolled copies.",
        ],
        ConfigRow::LoopInterchange => vec![
            "What: Enables loop interchange (-mllvm -enable-loopinterchange).",
            "Why: Reorders nested loop dimensions for better memory access.",
            "Tip: Useful for matrix/stencil benchmarks with column-major access.",
        ],
        ConfigRow::LoopDistribute => vec![
            "What: Enables loop distribution (-mllvm -enable-loop-distribute).",
            "Why: Splits loops with mixed dependences so vectorizable parts can proceed.",
            "Tip: Try when vectorization fails due to carried dependences.",
        ],
        ConfigRow::MarchNative => vec![
            "What: Targets the host CPU (-march=native).",
            "Why: Unlocks wider SIMD (AVX2/AVX-512/NEON) beyond the default target.",
            "Tip: Combine with Force Vec Width to test wider VFs on your hardware.",
        ],
        ConfigRow::ExtraCFlags => vec![
            "What: Appends extra clang C flags to compile/link commands.",
            "Why: Lets you quickly test hypotheses without changing code.",
            "Tip: Example: -fno-math-errno or -ffp-contract=fast",
        ],
        ConfigRow::ExtraLlvmFlags => vec![
            "What: Appends LLVM backend flags (each token passed via -mllvm).",
            "Why: Enables fine-grained pass tuning/diagnostics.",
            "Tip: Example: -debug-pass-manager",
        ],
    };

    for line in body {
        lines.push(Line::from(line.to_string()));
    }

    lines.push(Line::from(""));
    let workflow_hint = if app.is_config_text_editing() {
        "Workflow: You are editing text; Enter commits this field."
    } else {
        "Workflow: For pass tracking, try -O1 with vectorizers off, then re-enable selectively."
    };
    lines.push(Line::from(workflow_hint.to_string()));
    lines
}

fn render_benchmark_detail_page(frame: &mut Frame, app: &mut AppState) {
    let area = frame.area();
    if area.width < 100 || area.height < 30 {
        frame.render_widget(
            Paragraph::new(format!(
                "Terminal too small (100x30 minimum). Current: {}x{}",
                area.width, area.height
            )),
            area,
        );
        return;
    }

    let vertical = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ]);
    let [header_area, main_area, footer_area] = vertical.areas(area);
    render_detail_header(frame, app, header_area);

    let cols = Layout::horizontal([Constraint::Percentage(23), Constraint::Percentage(77)]);
    let [selector_area, right_area] = cols.areas(main_area);
    let right_rows = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(DETAIL_ATTR_BAR_HEIGHT),
    ]);
    let [code_area, attr_area] = right_rows.areas(right_area);
    app.set_detail_code_viewport_lines(code_area.height.saturating_sub(2));

    render_pass_selector_panel(frame, app, selector_area);
    render_code_view_panel(frame, app, code_area);
    render_line_attributes_bar(frame, app, attr_area);
    render_detail_footer(frame, app, footer_area);
    if app.is_pass_info_modal_open() {
        render_pass_info_modal(frame, app);
    }
    if app.is_side_by_side_diff_open() {
        render_side_by_side_diff_modal(frame, app);
    }
    if app.is_c_source_popup_open() {
        render_c_source_modal(frame, app);
    }
}

fn render_list_header(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let selected = app
        .selected_benchmark()
        .map(|b| b.name.as_str())
        .unwrap_or("-");
    let line = Line::from(vec![
        "TSVC TUI  ".into(),
        "Page: ".gray(),
        "Benchmark List".yellow(),
        "  Selected: ".gray(),
        selected.green(),
        "  Count: ".gray(),
        app.benchmarks.len().to_string().cyan(),
    ]);
    let header = Paragraph::new(line).block(Block::bordered().title("Session"));
    frame.render_widget(header, area);
}

fn render_detail_header(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let benchmark_name = app
        .selected_benchmark()
        .map(|b| b.name.as_str())
        .unwrap_or("-");
    let loop_id = app.selected_function_loop_id().unwrap_or("-");
    let config_label = app.current_compiler_config().label();

    let session = app.active_session_for_selected_benchmark();
    let (verdict_text, verdict_color) = session
        .map(format_verdict)
        .unwrap_or_else(|| ("\u{2014}".to_string(), Color::DarkGray));

    let left = format!("{benchmark_name} \u{00b7} {loop_id} \u{00b7} {config_label}");
    let line = Line::from(vec![
        Span::raw(left),
        Span::raw("     "),
        Span::styled(
            verdict_text,
            Style::default()
                .fg(verdict_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let header = Paragraph::new(line).block(Block::bordered().title("Detail"));
    frame.render_widget(header, area);
}

fn render_pass_selector_panel(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let title = if app.is_selector_focused() {
        format!(
            "Pass Timeline: {} [Focus]",
            app.pass_timeline_filter.label()
        )
    } else {
        format!("Pass Timeline: {}", app.pass_timeline_filter.label())
    };

    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("Analysis will start when you enter detail.")
                .block(Block::bordered().title(title.as_str())),
            area,
        );
        return;
    };

    if session.analysis_state == AnalysisState::Running {
        frame.render_widget(
            Paragraph::new("\u{27f3} Analyzing...").block(Block::bordered().title(title.as_str())),
            area,
        );
        return;
    }

    let stages = AppState::ordered_stages_with_counts(session, app.pass_timeline_filter);
    if stages.is_empty() {
        frame.render_widget(
            Paragraph::new("Waiting for pass timeline...")
                .block(Block::bordered().title(title.as_str())),
            area,
        );
        return;
    }

    let selected_stage = app.selected_stage;
    let selected_pass = app.selected_pass_index_in_stage(session);
    let mut selected_display_idx = None;

    let mut items = Vec::new();
    for (stage, count) in stages {
        items.push(
            ListItem::new(format!("{}  ({count})", stage.ui_label())).style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
        );

        let passes = AppState::pass_trace_for_stage(session, stage, app.pass_timeline_filter);
        for (pass_idx, entry) in passes.iter().enumerate() {
            let text = format!(
                "  {}  [{}]{}",
                pass_display_name(&entry.pass_key),
                timeline_status_label(entry),
                timeline_remark_suffix(entry, &session.remarks)
            );
            if stage == selected_stage && pass_idx == selected_pass {
                selected_display_idx = Some(items.len());
            }
            items.push(ListItem::new(text));
        }
    }

    let list = List::new(items)
        .block(Block::bordered().title(title.as_str()))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(selected_display_idx);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_code_view_panel(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let title = if app.is_code_view_focused() {
        format!("Inspector: {} [Focus]", app.code_view_mode.label())
    } else {
        format!("Inspector: {}", app.code_view_mode.label())
    };

    match app.code_view_mode {
        CodeViewMode::IrPostPass => render_ir_post_panel(frame, app, area, &title),
        CodeViewMode::IrDiff => render_ir_diff_panel(frame, app, area, &title),
    }
}

fn render_line_attributes_bar(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let text = Text::from(pass_summary_lines(app));
    let widget = Paragraph::new(text)
        .block(Block::bordered().title("Pass Summary"))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

fn pass_summary_lines(app: &AppState) -> Vec<Line<'static>> {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        return vec![Line::from(
            "Analysis results are required to inspect the selected pass.",
        )];
    };
    let Some(entry) = app.selected_trace_entry_in_stage(session) else {
        return vec![Line::from("Select a pass to inspect.")];
    };

    let mut lines = vec![
        Line::from(format!("Status: {}", entry.status.ui_label())),
        Line::from(format!(
            "Target: {}",
            if entry.target_raw.is_empty() {
                "(unknown-target)"
            } else {
                entry.target_raw.as_str()
            }
        )),
    ];

    if let Some(remark) = entry
        .remark_indices
        .iter()
        .find_map(|idx| session.remarks.get(*idx))
    {
        let message = remark.message.as_deref().unwrap_or(remark.name.as_str());
        lines.push(Line::from(format!("Reason: [{}] {}", remark.kind, message)));
    } else {
        lines.push(Line::from("Reason: No explicit reason emitted"));
    }

    lines
}

fn selected_ir_style(tag: ChangeTag, selected: bool) -> Style {
    if !selected {
        return match tag {
            ChangeTag::Insert => Style::default().fg(Color::White).bg(IR_INSERT_BG),
            ChangeTag::Delete => Style::default().fg(Color::White).bg(IR_DELETE_BG),
            ChangeTag::Equal => Style::default().fg(CODE_TEXT_FG).bg(CODE_BG),
        };
    }

    match tag {
        ChangeTag::Insert => Style::default()
            .fg(Color::White)
            .bg(IR_INSERT_SELECTED_BG)
            .add_modifier(Modifier::BOLD),
        ChangeTag::Delete => Style::default()
            .fg(Color::White)
            .bg(IR_DELETE_SELECTED_BG)
            .add_modifier(Modifier::BOLD),
        ChangeTag::Equal => Style::default()
            .fg(Color::White)
            .bg(IR_SELECTED_BG)
            .add_modifier(Modifier::BOLD),
    }
}

fn render_detail_source_panel(
    frame: &mut Frame,
    app: &AppState,
    area: ratatui::layout::Rect,
    title: &str,
) {
    let Some(source_text) = app.detail_source_text_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("Source not available").block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    if source_text.trim().is_empty() {
        frame.render_widget(
            Paragraph::new("(source not available)").block(Block::bordered().title(title)),
            area,
        );
        return;
    }

    // Collect highlighted source lines from current step's source_line_map + visible IR range
    let highlighted_lines = collect_highlighted_source_lines(app);
    let highlighted_source = syntax::highlight(SyntaxLang::C, &source_text);

    let lines: Vec<Line> = source_text
        .lines()
        .enumerate()
        .map(|(i, l)| {
            let line_num = (i + 1) as u32;
            let line_emphasis_style = if highlighted_lines.contains(&line_num) {
                Some(Style::default().bg(SOURCE_LINE_HIGHLIGHT_BG))
            } else {
                None
            };
            let prefix_char = if line_emphasis_style.is_some() {
                "*"
            } else {
                " "
            };
            let prefix = format!("{prefix_char}{:>3}| ", line_num);
            let prefix_style = if let Some(base) = line_emphasis_style {
                base.patch(Style::default().fg(Color::Yellow))
            } else {
                Style::default().bg(CODE_BG).fg(CODE_TEXT_FG)
            };
            let highlighted = highlighted_source.get(i).map(Vec::as_slice);

            prefixed_highlighted_line(&prefix, prefix_style, highlighted, l, line_emphasis_style)
        })
        .collect();

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title(title))
        .style(Style::default().bg(CODE_BG).fg(CODE_TEXT_FG))
        .scroll((app.source_detail_scroll, 0));
    frame.render_widget(paragraph, area);
}

fn render_c_source_modal(frame: &mut Frame, app: &AppState) {
    let area = centered_rect(frame.area(), 88, 82);
    frame.render_widget(Clear, area);

    let vertical = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]);
    let [content_area, footer_area] = vertical.areas(area);
    render_detail_source_panel(frame, app, content_area, "C Source");

    let hint = Paragraph::new(Text::from(vec![
        Line::from("C Source Popup"),
        Line::from("\u{2191}\u{2193} scroll · c close · esc close"),
    ]))
    .block(Block::bordered().title("Modal"));
    frame.render_widget(hint, footer_area);
}

/// Collect source line numbers that correspond to currently visible IR lines.
fn collect_highlighted_source_lines(app: &AppState) -> HashSet<u32> {
    let mut result = HashSet::new();

    let Some(session) = app.active_session_for_selected_benchmark() else {
        return result;
    };
    let Some(step) = app.selected_step_in_stage(session) else {
        return result;
    };
    if step.source_line_map.is_empty() {
        return result;
    }

    let start = app.ir_scroll as usize;
    // Estimate visible height as ~20 lines (will be adjusted by actual terminal size)
    let end = (start + 40).min(step.source_line_map.len());
    for line in step.source_line_map[start..end].iter().flatten() {
        result.insert(*line);
    }
    result
}

fn render_ir_diff_panel(
    frame: &mut Frame,
    app: &AppState,
    area: ratatui::layout::Rect,
    title: &str,
) {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("Waiting for analysis...").block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    let Some(step) = app.selected_step_in_stage(session) else {
        let hint = ir_unavailable_hint(app, session);
        frame.render_widget(
            Paragraph::new(hint).block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    if step.ir_lines.is_empty() {
        // Fallback: show unified diff text if ir_lines not populated
        let lines: Vec<Line> = step.diff_text.lines().map(|l| color_diff_line(l)).collect();
        let paragraph = Paragraph::new(Text::from(lines))
            .block(Block::bordered().title(title))
            .style(Style::default().bg(CODE_BG).fg(CODE_TEXT_FG))
            .scroll((app.ir_scroll, 0));
        frame.render_widget(paragraph, area);
        return;
    }

    let selected_idx = app.selected_ir_visible_index();
    let ir_text = step
        .ir_lines
        .iter()
        .map(|ir_line| ir_line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let highlighted_ir = syntax::highlight(SyntaxLang::LlvmIr, &ir_text);

    let lines: Vec<Line> = step
        .ir_lines
        .iter()
        .enumerate()
        .map(|(idx, ir_line)| {
            let selected = idx == selected_idx;
            if ir_line.is_source_annotation {
                let annotation_bg = match ir_line.tag {
                    ChangeTag::Insert if selected => IR_INSERT_SELECTED_BG,
                    ChangeTag::Delete if selected => IR_DELETE_SELECTED_BG,
                    ChangeTag::Equal if selected => IR_SELECTED_BG,
                    ChangeTag::Insert => IR_INSERT_BG,
                    ChangeTag::Delete => IR_DELETE_BG,
                    ChangeTag::Equal => CODE_BG,
                };
                let style = Style::default()
                    .fg(SOURCE_ANNOTATION_FG)
                    .bg(annotation_bg)
                    .add_modifier(if selected {
                        Modifier::ITALIC | Modifier::BOLD
                    } else {
                        Modifier::ITALIC
                    });
                let marker = if selected { ">" } else { " " };
                return Line::from(Span::styled(
                    format!("{marker}  {:>4} | {}", idx + 1, ir_line.text),
                    style,
                ));
            }

            let prefix = match ir_line.tag {
                ChangeTag::Insert => "+ ",
                ChangeTag::Delete => "- ",
                ChangeTag::Equal => "  ",
            };
            let marker = if selected { ">" } else { " " };
            let line_style = selected_ir_style(ir_line.tag, selected);
            let gutter = format!("{marker}{prefix}{:>4} | ", idx + 1);

            let highlighted = highlighted_ir.get(idx).map(Vec::as_slice);
            prefixed_highlighted_line(
                &gutter,
                line_style,
                highlighted,
                &ir_line.text,
                Some(line_style),
            )
        })
        .collect();

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title(title))
        .style(Style::default().bg(CODE_BG).fg(CODE_TEXT_FG))
        .scroll((app.ir_scroll, 0));
    frame.render_widget(paragraph, area);
}

fn render_ir_post_panel(
    frame: &mut Frame,
    app: &AppState,
    area: ratatui::layout::Rect,
    title: &str,
) {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("Waiting for analysis...").block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    let Some(step) = app.selected_step_in_stage(session) else {
        let hint = ir_unavailable_hint(app, session);
        frame.render_widget(
            Paragraph::new(hint).block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    let filtered_lines: Vec<_> = step
        .ir_lines
        .iter()
        .filter(|ir_line| !matches!(ir_line.tag, ChangeTag::Delete))
        .collect();

    if filtered_lines.is_empty() {
        frame.render_widget(
            Paragraph::new("No IR lines for this pass").block(Block::bordered().title(title)),
            area,
        );
        return;
    }

    let selected_idx = app.selected_ir_visible_index();
    let ir_text = filtered_lines
        .iter()
        .map(|ir_line| ir_line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let highlighted_ir = syntax::highlight(SyntaxLang::LlvmIr, &ir_text);

    let lines: Vec<Line> = filtered_lines
        .iter()
        .enumerate()
        .map(|(idx, ir_line)| {
            let selected = idx == selected_idx;
            if ir_line.is_source_annotation {
                let style = Style::default()
                    .fg(SOURCE_ANNOTATION_FG)
                    .bg(if selected { IR_SELECTED_BG } else { CODE_BG })
                    .add_modifier(if selected {
                        Modifier::ITALIC | Modifier::BOLD
                    } else {
                        Modifier::ITALIC
                    });
                let marker = if selected { ">" } else { " " };
                return Line::from(Span::styled(
                    format!("{marker}  {:>4} | {}", idx + 1, ir_line.text),
                    style,
                ));
            }

            let base_style = selected_ir_style(ChangeTag::Equal, selected);
            let marker = if selected { ">" } else { " " };
            let gutter = format!("{marker}  {:>4} | ", idx + 1);
            let highlighted = highlighted_ir.get(idx).map(Vec::as_slice);
            prefixed_highlighted_line(
                &gutter,
                base_style,
                highlighted,
                &ir_line.text,
                Some(base_style),
            )
        })
        .collect();

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title(title))
        .style(Style::default().bg(CODE_BG).fg(CODE_TEXT_FG))
        .scroll((app.ir_scroll, 0));
    frame.render_widget(paragraph, area);
}

fn render_pass_info_modal(frame: &mut Frame, app: &AppState) {
    let area = centered_rect(frame.area(), 82, 78);
    frame.render_widget(Clear, area);
    let vertical = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]);
    let [content_area, footer_area] = vertical.areas(area);

    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("Waiting for analysis...").block(Block::bordered().title("Pass Info")),
            content_area,
        );
        return;
    };
    let Some(entry) = app.selected_trace_entry_in_stage(session) else {
        let hint = match session.analysis_state {
            AnalysisState::None => "Waiting for analysis...",
            AnalysisState::Running => "\u{27f3} Analyzing...",
            AnalysisState::Ready => "Select a pass",
            AnalysisState::Failed => "Analysis failed",
        };
        frame.render_widget(
            Paragraph::new(hint).block(Block::bordered().title("Pass Info")),
            content_area,
        );
        return;
    };

    let mut lines = vec![
        Line::from("Summary"),
        Line::from(format!(
            "Pass: {}",
            if entry.pass.is_empty() {
                entry.pass_key.as_str()
            } else {
                entry.pass.as_str()
            }
        )),
        Line::from(format!("Status: {}", entry.status.ui_label())),
        Line::from(format!("Occurrence: #{}", entry.pass_occurrence)),
        Line::from(format!(
            "Target: {}",
            if entry.target_raw.is_empty() {
                "(unknown-target)"
            } else {
                entry.target_raw.as_str()
            }
        )),
        Line::from(""),
        Line::from("Decision"),
    ];

    let remark_lines = format_trace_remarks(entry, &session.remarks);
    if remark_lines.is_empty() {
        lines.push(Line::from("No explicit reason emitted"));
    } else {
        lines.extend(remark_lines);
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Raw Log"));
    if entry.log_lines.is_empty() {
        lines.push(Line::from("(none)"));
    } else {
        for line in &entry.log_lines {
            lines.push(Line::from(line.clone()));
        }
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title("Pass Info"))
        .style(Style::default().bg(CODE_BG).fg(CODE_TEXT_FG))
        .scroll((app.pass_info_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, content_area);

    let footer = Paragraph::new(Text::from(vec![
        Line::from("Pass Info Modal"),
        Line::from("\u{2191}\u{2193} scroll · esc close"),
    ]))
    .block(Block::bordered().title("Modal"));
    frame.render_widget(footer, footer_area);
}

fn render_benchmarks(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
        .benchmarks
        .iter()
        .map(|b| {
            let main_text = format!("{}  {}  {}", b.name, b.category, b.data_type);
            match app.verdict_badge_for_benchmark(&b.name) {
                Some((text, color)) => ListItem::new(Line::from(vec![
                    Span::raw(main_text),
                    Span::raw("  "),
                    Span::styled(text, Style::default().fg(color)),
                ])),
                None => ListItem::new(Line::from(main_text)),
            }
        })
        .collect();

    let list_title = if app.is_benchmarks_focused() {
        "Benchmarks [Focus]"
    } else {
        "Benchmarks"
    };
    let list = List::new(items)
        .block(Block::bordered().title(list_title))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    if !app.benchmarks.is_empty() {
        state.select(Some(app.selected_idx));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_benchmark_source_code(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let title = if app.is_source_code_focused() {
        "C Source (kernel-focused) [Focus]"
    } else {
        "C Source (kernel-focused)"
    };

    let Some(benchmark) = app.selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No benchmark selected.").block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    let lines = if benchmark.source_code.trim().is_empty() {
        vec![Line::from("No source text available.")]
    } else {
        let highlighted = syntax::highlight(SyntaxLang::C, &benchmark.source_code);
        benchmark
            .source_code
            .lines()
            .enumerate()
            .map(|(i, line)| highlighted_line(highlighted.get(i).map(Vec::as_slice), line, None))
            .collect()
    };

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title(title))
        .style(Style::default().bg(CODE_BG).fg(CODE_TEXT_FG))
        .scroll((app.list_source_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_list_footer(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let hints = "q quit | \u{2190}\u{2192} section | \u{2191}\u{2193} select/scroll | enter select function | c config";
    let text = Text::from(vec![
        Line::from(hints),
        Line::from(format!("Status: {}", app.status_message)),
    ]);

    let footer = Paragraph::new(text).block(Block::bordered().title("Keys"));
    frame.render_widget(footer, area);
}

fn render_detail_footer(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let hints = "\u{2190}\u{2192} section \u{00b7} \u{2191}\u{2193} pass/cursor \u{00b7} tab/s-tab IR mode \u{00b7} enter pass info \u{00b7} r timeline filter \u{00b7} c C popup \u{00b7} d side-by-side diff \u{00b7} y copy";
    let text = Text::from(vec![
        Line::from(hints),
        Line::from(format!("Status: {}", app.status_message)),
    ]);

    let footer = Paragraph::new(text).block(Block::bordered().title("Keys"));
    frame.render_widget(footer, area);
}

fn render_function_select_modal(frame: &mut Frame, app: &AppState) {
    let area = centered_rect(frame.area(), 60, 60);
    frame.render_widget(Clear, area);

    let vertical = Layout::vertical([Constraint::Min(0), Constraint::Length(2)]);
    let [list_area, hint_area] = vertical.areas(area);

    let items = app
        .function_modal_items_for_selected_benchmark()
        .unwrap_or(&[])
        .iter()
        .map(|function| ListItem::new(format!("{} ({})", function.loop_id, function.symbol)))
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(Block::bordered().title("Select Function"))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    if !app
        .function_modal_items_for_selected_benchmark()
        .unwrap_or(&[])
        .is_empty()
    {
        state.select(Some(app.function_modal_selected_idx));
    }
    frame.render_stateful_widget(list, list_area, &mut state);

    let hint = Paragraph::new("\u{2191}\u{2193} move | enter open detail | esc cancel")
        .block(Block::bordered().title("Modal"));
    frame.render_widget(hint, hint_area);
}

fn render_side_by_side_diff_modal(frame: &mut Frame, app: &AppState) {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        return;
    };
    let Some(step) = app.selected_step_in_stage(session) else {
        return;
    };

    let rows = build_side_by_side_diff_rows(step);
    let area = centered_rect(frame.area(), 94, 86);
    frame.render_widget(Clear, area);

    let vertical = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]);
    let [content_area, footer_area] = vertical.areas(area);
    let horizontal = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]);
    let [left_area, right_area] = horizontal.areas(content_area);

    let mut left_line_no = 0usize;
    let mut right_line_no = 0usize;
    let left_lines: Vec<Line> = rows
        .iter()
        .map(|row| render_side_by_side_cell(row.left, &mut left_line_no))
        .collect();
    let right_lines: Vec<Line> = rows
        .iter()
        .map(|row| render_side_by_side_cell(row.right, &mut right_line_no))
        .collect();

    let left = Paragraph::new(Text::from(left_lines))
        .block(Block::bordered().title("Before"))
        .style(Style::default().bg(CODE_BG).fg(CODE_TEXT_FG))
        .scroll((app.side_by_side_diff_scroll, 0));
    frame.render_widget(left, left_area);

    let right = Paragraph::new(Text::from(right_lines))
        .block(Block::bordered().title("After"))
        .style(Style::default().bg(CODE_BG).fg(CODE_TEXT_FG))
        .scroll((app.side_by_side_diff_scroll, 0));
    frame.render_widget(right, right_area);

    let hint = Paragraph::new(Text::from(vec![
        Line::from(format!(
            "Side-by-Side Diff · {}",
            pass_display_name(&step.pass_key)
        )),
        Line::from("\u{2191}\u{2193} scroll · d close · esc close"),
    ]))
    .block(Block::bordered().title("Modal"));
    frame.render_widget(hint, footer_area);
}

fn centered_rect(
    area: ratatui::layout::Rect,
    width_pct: u16,
    height_pct: u16,
) -> ratatui::layout::Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height_pct) / 2),
        Constraint::Percentage(height_pct),
        Constraint::Percentage((100 - height_pct) / 2),
    ]);
    let [_, middle, _] = vertical.areas(area);
    let horizontal = Layout::horizontal([
        Constraint::Percentage((100 - width_pct) / 2),
        Constraint::Percentage(width_pct),
        Constraint::Percentage((100 - width_pct) / 2),
    ]);
    let [_, centered, _] = horizontal.areas(middle);
    centered
}

fn highlighted_line(
    highlighted: Option<&[StyledChunk]>,
    plain: &str,
    overlay_style: Option<Style>,
) -> Line<'static> {
    let mut spans = Vec::new();
    append_highlighted_spans(&mut spans, highlighted, plain, overlay_style);
    Line::from(spans)
}

fn prefixed_highlighted_line(
    prefix: &str,
    prefix_style: Style,
    highlighted: Option<&[StyledChunk]>,
    plain: &str,
    overlay_style: Option<Style>,
) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(Span::styled(prefix.to_string(), prefix_style));
    append_highlighted_spans(&mut spans, highlighted, plain, overlay_style);
    Line::from(spans)
}

fn append_highlighted_spans(
    spans: &mut Vec<Span<'static>>,
    highlighted: Option<&[StyledChunk]>,
    plain: &str,
    overlay_style: Option<Style>,
) {
    let style_with_overlay =
        |style: Style| overlay_style.map_or(style, |overlay| overlay.patch(style));

    if let Some(chunks) = highlighted
        && !chunks.is_empty()
    {
        for chunk in chunks {
            spans.push(Span::styled(
                chunk.text.clone(),
                style_with_overlay(chunk.style),
            ));
        }
        return;
    }

    if !plain.is_empty() || spans.is_empty() {
        spans.push(Span::styled(
            plain.to_string(),
            style_with_overlay(Style::default()),
        ));
    }
}

fn build_side_by_side_diff_rows(step: &AnalysisStep) -> Vec<SideBySideDiffRow<'_>> {
    let mut rows = Vec::new();
    let mut pending_left = Vec::new();
    let mut pending_right = Vec::new();

    for ir_line in &step.ir_lines {
        match ir_line.tag {
            ChangeTag::Equal => {
                flush_side_by_side_block(&mut rows, &mut pending_left, &mut pending_right);
                rows.push(SideBySideDiffRow {
                    left: Some(ir_line),
                    right: Some(ir_line),
                });
            }
            ChangeTag::Delete => pending_left.push(ir_line),
            ChangeTag::Insert => pending_right.push(ir_line),
        }
    }

    flush_side_by_side_block(&mut rows, &mut pending_left, &mut pending_right);
    rows
}

fn flush_side_by_side_block<'a>(
    rows: &mut Vec<SideBySideDiffRow<'a>>,
    pending_left: &mut Vec<&'a crate::core::model::IrLine>,
    pending_right: &mut Vec<&'a crate::core::model::IrLine>,
) {
    let block_len = pending_left.len().max(pending_right.len());
    for idx in 0..block_len {
        rows.push(SideBySideDiffRow {
            left: pending_left.get(idx).copied(),
            right: pending_right.get(idx).copied(),
        });
    }
    pending_left.clear();
    pending_right.clear();
}

fn render_side_by_side_cell(
    ir_line: Option<&crate::core::model::IrLine>,
    line_no: &mut usize,
) -> Line<'static> {
    let empty_style = Style::default().bg(CODE_BG).fg(CODE_TEXT_FG);
    let Some(ir_line) = ir_line else {
        return Line::from(Span::styled("      |".to_string(), empty_style));
    };

    let (prefix, style) = if ir_line.is_source_annotation {
        let bg = match ir_line.tag {
            ChangeTag::Insert => IR_INSERT_BG,
            ChangeTag::Delete => IR_DELETE_BG,
            ChangeTag::Equal => CODE_BG,
        };
        (
            String::from(" src  | "),
            Style::default()
                .fg(SOURCE_ANNOTATION_FG)
                .bg(bg)
                .add_modifier(Modifier::ITALIC),
        )
    } else {
        *line_no += 1;
        (
            format!("{:>5} | ", *line_no),
            match ir_line.tag {
                ChangeTag::Insert => Style::default().fg(Color::White).bg(IR_INSERT_BG),
                ChangeTag::Delete => Style::default().fg(Color::White).bg(IR_DELETE_BG),
                ChangeTag::Equal => empty_style,
            },
        )
    };

    Line::from(Span::styled(format!("{prefix}{}", ir_line.text), style))
}

// --- Helper functions ---

fn ir_unavailable_hint(app: &AppState, session: &RunSession) -> &'static str {
    if let Some(entry) = app.selected_trace_entry_in_stage(session)
        && !entry.status.changed()
    {
        return "IR unchanged after this pass";
    }
    match session.analysis_state {
        AnalysisState::None => "Waiting for analysis...",
        AnalysisState::Running => "\u{27f3} Analyzing...",
        AnalysisState::Ready => "Select a pass",
        AnalysisState::Failed => "Analysis failed",
    }
}

fn format_trace_remarks(entry: &PassTraceEntry, remarks: &[RemarkEntry]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for idx in &entry.remark_indices {
        let Some(remark) = remarks.get(*idx) else {
            continue;
        };
        let message = remark.message.as_deref().unwrap_or(remark.name.as_str());
        lines.push(Line::from(format!(
            "[{}] {}::{} {}",
            remark.kind, remark.pass, remark.name, message
        )));
    }
    lines
}

fn timeline_status_label(entry: &PassTraceEntry) -> &'static str {
    match entry.status {
        PassTraceStatus::Changed => "changed",
        PassTraceStatus::RanNoChange => "ran",
    }
}

fn timeline_remark_suffix(entry: &PassTraceEntry, remarks: &[RemarkEntry]) -> String {
    if entry.remark_indices.is_empty() {
        return String::new();
    }
    let mut badges = Vec::new();
    for idx in &entry.remark_indices {
        let Some(remark) = remarks.get(*idx) else {
            continue;
        };
        let badge = match remark.kind {
            RemarkKind::Passed => "✓",
            RemarkKind::Missed => "✗",
            RemarkKind::Analysis | RemarkKind::Other => "·",
        };
        if !badges.iter().any(|existing| existing == &badge) {
            badges.push(badge);
        }
    }
    if badges.is_empty() {
        String::new()
    } else {
        format!(" {}", badges.join(""))
    }
}

fn pass_display_name(pass_key: &str) -> &str {
    match pass_key {
        "licm" => "Loop Invariant CM",
        "loopvectorize" => "Loop Vectorize",
        "slpvectorizer" => "SLP Vectorize",
        "indvars" => "IndVar Simplify",
        "looprotate" => "Loop Rotation",
        "loopunroll" => "Loop Unroll",
        "instcombine" => "Instruction Combine",
        "sroa" => "Scalar Replacement",
        "inline" => "Function Inlining",
        "gvn" => "Global Value Number",
        "dce" => "Dead Code Elim",
        "simplifycfg" => "Control Flow Simplify",
        "earlycse" => "Common Subexpr Elim",
        "mem2reg" => "Memory to Register",
        "loopsimplify" => "Loop Canonicalize",
        _ => pass_key,
    }
}

fn format_verdict(session: &RunSession) -> (String, Color) {
    match session.analysis_state {
        AnalysisState::Running => ("\u{27f3} Analyzing...".to_string(), Color::Cyan),
        AnalysisState::Failed => ("\u{2717} Analysis Failed".to_string(), Color::Red),
        AnalysisState::Ready => {
            let summary = &session.remarks_summary;
            if summary.vectorized > 0 {
                let vf = extract_vf_from_session(session);
                let verdict = match vf {
                    Some(n) => format!("\u{2713} VECTORIZED  VF={n}"),
                    None => "\u{2713} VECTORIZED".to_string(),
                };
                (verdict, Color::Green)
            } else if summary.missed_details > 0 {
                ("\u{2717} NOT VECTORIZED".to_string(), Color::Red)
            } else if summary.not_beneficial > 0 {
                ("\u{25cb} SKIPPED".to_string(), Color::Yellow)
            } else if has_vectorizer_ir_changes(session) {
                ("~ LIKELY VECTORIZED".to_string(), Color::Cyan)
            } else {
                ("\u{2014}".to_string(), Color::DarkGray)
            }
        }
        AnalysisState::None => ("\u{2014}".to_string(), Color::DarkGray),
    }
}

fn extract_vf_from_session(session: &RunSession) -> Option<u32> {
    for r in &session.remarks {
        if r.pass == "loop-vectorize"
            && let Some(msg) = &r.message
        {
            for pattern in &["VF = ", "VF="] {
                if let Some(pos) = msg.find(pattern) {
                    let rest = msg[pos + pattern.len()..].trim_start_matches(' ');
                    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = num.parse::<u32>() {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

fn color_diff_line(line: &str) -> Line<'_> {
    if line.starts_with("+++") || line.starts_with("---") {
        Line::from(line.to_string())
    } else if line.starts_with('+') {
        Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Green),
        ))
    } else if line.starts_with('-') {
        Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Red),
        ))
    } else if line.starts_with("@@") {
        Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Cyan),
        ))
    } else {
        Line::from(line.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{AnalysisSource, AnalysisStage, IrLine, IrLineDetails};

    fn step_with_lines(lines: Vec<IrLine>) -> AnalysisStep {
        AnalysisStep {
            visible_index: 0,
            raw_index: 1,
            pass: String::from("LoopVectorizePass"),
            pass_key: String::from("loopvectorize"),
            pass_occurrence: 1,
            stage: AnalysisStage::Vectorize,
            target_raw: String::from("s161"),
            target_function: Some(String::from("s161")),
            changed_lines: 2,
            diff_text: String::new(),
            ir_lines: lines,
            source_line_map: vec![],
            remark_indices: vec![],
            source: AnalysisSource::TraceFast,
        }
    }

    fn ir_line(tag: ChangeTag, text: &str) -> IrLine {
        IrLine {
            tag,
            text: text.to_string(),
            is_source_annotation: false,
            details: IrLineDetails::default(),
        }
    }

    #[test]
    fn side_by_side_rows_pair_deleted_and_inserted_blocks() {
        let step = step_with_lines(vec![
            ir_line(ChangeTag::Equal, "header"),
            ir_line(ChangeTag::Delete, "old1"),
            ir_line(ChangeTag::Delete, "old2"),
            ir_line(ChangeTag::Insert, "new1"),
            ir_line(ChangeTag::Equal, "tail"),
        ]);

        let rows = build_side_by_side_diff_rows(&step);

        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].left.map(|line| line.text.as_str()), Some("header"));
        assert_eq!(rows[0].right.map(|line| line.text.as_str()), Some("header"));
        assert_eq!(rows[1].left.map(|line| line.text.as_str()), Some("old1"));
        assert_eq!(rows[1].right.map(|line| line.text.as_str()), Some("new1"));
        assert_eq!(rows[2].left.map(|line| line.text.as_str()), Some("old2"));
        assert_eq!(rows[2].right.map(|line| line.text.as_str()), None);
        assert_eq!(rows[3].left.map(|line| line.text.as_str()), Some("tail"));
        assert_eq!(rows[3].right.map(|line| line.text.as_str()), Some("tail"));
    }

    #[test]
    fn side_by_side_rows_keep_equal_annotations_in_both_columns() {
        let mut annotation = ir_line(ChangeTag::Equal, ";; for.body");
        annotation.is_source_annotation = true;
        let step = step_with_lines(vec![annotation]);

        let rows = build_side_by_side_diff_rows(&step);

        assert_eq!(rows.len(), 1);
        assert!(rows[0].left.expect("left annotation").is_source_annotation);
        assert!(
            rows[0]
                .right
                .expect("right annotation")
                .is_source_annotation
        );
    }
}
