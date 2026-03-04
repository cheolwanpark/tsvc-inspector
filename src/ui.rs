use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use similar::ChangeTag;

use crate::app::{AppState, has_vectorizer_ir_changes};
use crate::model::{
    AnalysisStage, AnalysisState, AnalysisStep, AppPage, RemarkEntry, RemarkKind, RunSession,
};
use crate::syntax::{self, StyledChunk, SyntaxLang};

const CODE_BG: Color = Color::Rgb(14, 20, 28);
const CODE_TEXT_FG: Color = Color::Gray;
const SOURCE_LINE_HIGHLIGHT_BG: Color = Color::Rgb(44, 52, 64);
const IR_INSERT_BG: Color = Color::Rgb(19, 70, 35);
const IR_DELETE_BG: Color = Color::Rgb(90, 28, 28);

pub fn render(frame: &mut Frame, app: &AppState) {
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

    let horizontal = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]);
    let [list_area, source_area] = horizontal.areas(main_area);
    render_benchmarks(frame, app, list_area);
    render_benchmark_source_code(frame, app, source_area);
    render_list_footer(frame, app, footer_area);
    if app.is_function_modal_open() {
        render_function_select_modal(frame, app);
    }
}

fn render_benchmark_detail_page(frame: &mut Frame, app: &AppState) {
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

    // 2x2 grid: top row (30%) and bottom row (70%)
    let rows = Layout::vertical([Constraint::Percentage(30), Constraint::Percentage(70)]);
    let [top_row, bottom_row] = rows.areas(main_area);

    // Top row: stage list (25%) | pass list (75%)
    let top_cols = Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)]);
    let [stage_area, pass_area] = top_cols.areas(top_row);

    // Bottom row: C source (35%) | IR view (65%)
    let bottom_cols = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]);
    let [source_area, ir_area] = bottom_cols.areas(bottom_row);

    render_stage_list(frame, app, stage_area);
    render_pass_list_panel(frame, app, pass_area);
    render_detail_source_panel(frame, app, source_area);
    render_ir_view_panel(frame, app, ir_area);
    render_detail_footer(frame, app, footer_area);
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
    let profile = app.active_profile.label();

    let session = app.active_session_for_selected_benchmark();
    let (verdict_text, verdict_color) = session
        .map(format_verdict)
        .unwrap_or_else(|| ("\u{2014}".to_string(), Color::DarkGray));

    let left = format!("{benchmark_name} \u{00b7} {loop_id} \u{00b7} {profile}");
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

fn render_stage_list(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let title = if app.is_stage_focused() {
        "Stages [Focus]"
    } else {
        "Stages"
    };

    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("Press 'a' to analyze").block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    if session.analysis_state == AnalysisState::Running {
        frame.render_widget(
            Paragraph::new("\u{27f3} Analyzing...").block(Block::bordered().title(title)),
            area,
        );
        return;
    }

    let stages = AppState::ordered_stages_with_counts(session);
    if stages.is_empty() {
        frame.render_widget(
            Paragraph::new("Press 'a' to analyze").block(Block::bordered().title(title)),
            area,
        );
        return;
    }

    let selected_pos = stages.iter().position(|(s, _)| *s == app.selected_stage);

    let items: Vec<ListItem> = stages
        .iter()
        .map(|(stage, count)| {
            let marker = if *stage == AnalysisStage::Vectorize {
                "\u{2605}"
            } else {
                " "
            };
            let text = format!("{marker} {}  ({})", stage.ui_label(), count);
            ListItem::new(text)
        })
        .collect();

    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(selected_pos);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_pass_list_panel(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let stage_label = app.selected_stage.ui_label();
    let is_vectorize = app.selected_stage == AnalysisStage::Vectorize;
    let star = if is_vectorize { " \u{2605}" } else { "" };
    let title_base = format!("Passes in {stage_label}{star}");
    let title = if app.is_pass_focused() {
        format!("{title_base} [Focus]")
    } else {
        title_base
    };

    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No analysis").block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    let passes = AppState::passes_for_stage(session, app.selected_stage);
    if passes.is_empty() {
        frame.render_widget(
            Paragraph::new("No passes in this stage").block(Block::bordered().title(title)),
            area,
        );
        return;
    }

    let selected_idx = app.selected_pass_index_in_stage(session);

    let items: Vec<ListItem> = passes
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let (icon, _msg) = pass_remark_summary(step, &session.remarks);
            let marker = if step.stage == AnalysisStage::Vectorize {
                "\u{2605}"
            } else {
                " "
            };
            let cursor = if i == selected_idx && app.is_ir_view_focused() {
                "\u{25c0}"
            } else {
                " "
            };
            let text = format!(
                "{marker} {}  {} [\u{0394}{}] {cursor}",
                pass_display_name(&step.pass_key),
                icon,
                step.changed_lines,
            );
            ListItem::new(text)
        })
        .collect();

    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(Some(selected_idx));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_detail_source_panel(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let title = if app.is_source_view_focused() {
        "C Source [Focus]"
    } else {
        "C Source"
    };

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

fn render_ir_view_panel(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let title = if app.is_ir_view_focused() {
        "IR View [Focus]"
    } else {
        "IR View"
    };

    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No analysis").block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    let Some(step) = app.selected_step_in_stage(session) else {
        let hint = match session.analysis_state {
            AnalysisState::None => "Press 'a' to analyze",
            AnalysisState::Running => "\u{27f3} Analyzing...",
            AnalysisState::Ready => "Select a pass",
            AnalysisState::Failed => "Analysis failed",
        };
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
            let (prefix, base_style) = match ir_line.tag {
                ChangeTag::Insert => ("+ ", Style::default().fg(Color::White).bg(IR_INSERT_BG)),
                ChangeTag::Delete => ("- ", Style::default().fg(Color::White).bg(IR_DELETE_BG)),
                ChangeTag::Equal => ("  ", Style::default().fg(CODE_TEXT_FG).bg(CODE_BG)),
            };

            let highlighted = highlighted_ir.get(idx).map(Vec::as_slice);
            prefixed_highlighted_line(
                prefix,
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
    let hints = "q quit | \u{2190}\u{2192} focus pane | \u{2191}\u{2193} select-or-scroll | enter select function";
    let text = Text::from(vec![
        Line::from(hints),
        Line::from(format!("Status: {}", app.status_message)),
    ]);

    let footer = Paragraph::new(text).block(Block::bordered().title("Keys"));
    frame.render_widget(footer, area);
}

fn render_detail_footer(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let hints = "Tab/S-Tab cycle pane \u{00b7} \u{2191}\u{2193} navigate \u{00b7} a analyze \u{00b7} r run \u{00b7} p profile \u{00b7} c clear";
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

    let hint = Paragraph::new("\u{2191}\u{2193} move | enter confirm | esc cancel")
        .block(Block::bordered().title("Modal"));
    frame.render_widget(hint, hint_area);
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

// --- Helper functions ---

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

fn pass_remark_summary(step: &AnalysisStep, remarks: &[RemarkEntry]) -> (char, String) {
    for idx in &step.remark_indices {
        let Some(r) = remarks.get(*idx) else {
            continue;
        };
        let icon = match r.kind {
            RemarkKind::Passed => '\u{2713}',
            RemarkKind::Missed => '\u{2717}',
            _ => '\u{2014}',
        };
        let msg = r.message.as_deref().unwrap_or(r.name.as_str()).to_string();
        return (icon, msg);
    }
    ('\u{2014}', String::from("no remarks"))
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
