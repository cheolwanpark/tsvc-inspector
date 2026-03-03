use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::AppState;
use crate::model::{AnalysisStage, AnalysisState, AnalysisStep, AppPage, RemarkEntry, RemarkKind, RunSession};

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
    if area.width < 80 || area.height < 24 {
        frame.render_widget(
            Paragraph::new(format!(
                "Terminal too small (80×24 minimum). Current: {}×{}",
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

    let horizontal = Layout::horizontal([Constraint::Percentage(22), Constraint::Percentage(78)]);
    let [stage_area, right_area] = horizontal.areas(main_area);
    render_stage_list(frame, app, stage_area);
    render_right_panel(frame, app, right_area);
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
        .unwrap_or_else(|| ("—".to_string(), Color::DarkGray));

    let left = format!("{benchmark_name} · {loop_id} · {profile}");
    let line = Line::from(vec![
        Span::raw(left),
        Span::raw("     "),
        Span::styled(verdict_text, Style::default().fg(verdict_color).add_modifier(Modifier::BOLD)),
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
            Paragraph::new("⟳ Analyzing...").block(Block::bordered().title(title)),
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

    let selected_pos = stages
        .iter()
        .position(|(s, _)| *s == app.selected_stage);

    let items: Vec<ListItem> = stages
        .iter()
        .map(|(stage, count)| {
            let marker = if *stage == AnalysisStage::Vectorize {
                "★"
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

fn render_right_panel(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let source_lines = app
        .selected_benchmark()
        .map(|b| b.source_code.lines().count())
        .unwrap_or(0);
    let source_h = (source_lines.clamp(3, 10) as u16) + 2;

    let pass_count = app
        .active_session_for_selected_benchmark()
        .map(|s| AppState::passes_for_stage(s, app.selected_stage).len())
        .unwrap_or(0);
    let pass_h = (pass_count.clamp(2, 8) as u16) + 2;

    let vertical = Layout::vertical([
        Constraint::Length(source_h),
        Constraint::Length(pass_h),
        Constraint::Min(5),
    ]);
    let [source_area, pass_area, detail_area] = vertical.areas(area);

    render_c_source_panel(frame, app, source_area);
    render_pass_list_panel(frame, app, pass_area);
    render_pass_detail_panel(frame, app, detail_area);
}

fn render_c_source_panel(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(benchmark) = app.selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("Source not available").block(Block::bordered().title("C Source")),
            area,
        );
        return;
    };

    let mut lines: Vec<Line> = if benchmark.source_code.trim().is_empty() {
        vec![Line::from("(source not available)".dark_gray())]
    } else {
        benchmark
            .source_code
            .lines()
            .map(|l| Line::from(l.to_string()))
            .collect()
    };

    let session = app.active_session_for_selected_benchmark();
    let runtime_line = if let Some(s) = session {
        if let Some(result) = s.loop_results.first() {
            format!(
                "Runtime: {:.3}ms · Checksum: {}",
                result.time_sec * 1000.0,
                result.checksum
            )
        } else {
            String::from("(not run yet)")
        }
    } else {
        String::from("(not run yet)")
    };

    lines.push(Line::from(Span::styled(
        "─".repeat(40),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        runtime_line,
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(Text::from(lines)).block(Block::bordered().title("C Source"));
    frame.render_widget(paragraph, area);
}

fn render_pass_list_panel(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let stage_label = app.selected_stage.ui_label();
    let is_vectorize = app.selected_stage == AnalysisStage::Vectorize;
    let star = if is_vectorize { " ★" } else { "" };
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
            Paragraph::new("No passes in this stage")
                .block(Block::bordered().title(title)),
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
                "★"
            } else {
                " "
            };
            let cursor = if i == selected_idx && app.is_diff_focused() {
                "◀"
            } else {
                " "
            };
            let text = format!(
                "{marker} {}  {} [Δ{}] {cursor}",
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

fn render_pass_detail_panel(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let title = if app.is_diff_focused() {
        "IR Diff [Focus]"
    } else {
        "IR Diff"
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
            AnalysisState::Running => "⟳ Analyzing...",
            AnalysisState::Ready => "Select a pass above",
            AnalysisState::Failed => "Analysis failed",
        };
        frame.render_widget(
            Paragraph::new(hint).block(Block::bordered().title(title)),
            area,
        );
        return;
    };

    let (icon, remark_msg) = pass_remark_summary(step, &session.remarks);
    let display_name = pass_display_name(&step.pass_key);

    let mut lines = vec![
        Line::from(Span::styled(
            display_name.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("{icon} {remark_msg}")),
        Line::from(Span::styled(
            "── IR Diff ↕ ─────────────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    if step.diff_text.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            "(no diff for this pass)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for line in step.diff_text.lines() {
            lines.push(color_diff_line(line));
        }
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title(title))
        .scroll((app.diff_scroll, 0));
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
        benchmark
            .source_code
            .lines()
            .map(|line| Line::from(line.to_string()))
            .collect()
    };

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title(title))
        .scroll((app.list_source_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_list_footer(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let hints = "q quit | ←→ focus pane | ↑↓ select-or-scroll | enter select function";
    let text = Text::from(vec![
        Line::from(hints),
        Line::from(format!("Status: {}", app.status_message)),
    ]);

    let footer = Paragraph::new(text).block(Block::bordered().title("Keys"));
    frame.render_widget(footer, area);
}

fn render_detail_footer(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let hints = "q quit · esc back · ←→ focus · ↑↓ navigate · ↵ confirm · a analyze · r run · p profile · c clear";
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

    let hint = Paragraph::new("↑↓ move | enter confirm | esc cancel")
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
        AnalysisState::Running => ("⟳ Analyzing...".to_string(), Color::Cyan),
        AnalysisState::Failed => ("✗ Analysis Failed".to_string(), Color::Red),
        AnalysisState::Ready => {
            let summary = &session.remarks_summary;
            if summary.vectorized > 0 {
                let vf = extract_vf_from_session(session);
                let verdict = match vf {
                    Some(n) => format!("✓ VECTORIZED  VF={n}"),
                    None => "✓ VECTORIZED".to_string(),
                };
                (verdict, Color::Green)
            } else if summary.missed_details > 0 {
                ("✗ NOT VECTORIZED".to_string(), Color::Red)
            } else if summary.not_beneficial > 0 {
                ("○ SKIPPED".to_string(), Color::Yellow)
            } else {
                ("—".to_string(), Color::DarkGray)
            }
        }
        AnalysisState::None => ("—".to_string(), Color::DarkGray),
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
                    let num: String =
                        rest.chars().take_while(|c| c.is_ascii_digit()).collect();
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
            RemarkKind::Passed => '✓',
            RemarkKind::Missed => '✗',
            _ => '—',
        };
        let msg = r
            .message
            .as_deref()
            .unwrap_or(r.name.as_str())
            .to_string();
        return (icon, msg);
    }
    ('—', String::from("no remarks"))
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
