use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{AppState, JobState};
use crate::model::AppPage;

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
    let vertical = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ]);
    let [header_area, main_area, footer_area] = vertical.areas(frame.area());
    render_detail_header(frame, app, header_area);

    let horizontal = Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)]);
    let [steps_area, diff_area] = horizontal.areas(main_area);
    render_ir_diff_steps(frame, app, steps_area);
    render_ir_diff(frame, app, diff_area);
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
    let selected = app
        .selected_benchmark()
        .map(|b| b.name.as_str())
        .unwrap_or("-");
    let job = match app.job_state {
        JobState::Idle => "Idle".to_string(),
        JobState::Running(kind) => format!("Running {kind}"),
    };

    let line = Line::from(vec![
        "TSVC TUI  ".into(),
        "Page: ".gray(),
        "Benchmark Detail".yellow(),
        "  Benchmark: ".gray(),
        selected.green(),
        "  Function: ".gray(),
        app.selected_function_loop_id().unwrap_or("-").green(),
        " (".gray(),
        app.selected_function_symbol().unwrap_or("-").gray(),
        ")".gray(),
        "  Profile: ".gray(),
        app.active_profile.to_string().cyan(),
        "  Mode: ".gray(),
        app.function_run_mode.to_string().cyan(),
        "  Focus: ".gray(),
        app.detail_focus.label().cyan(),
        "  Job: ".gray(),
        job.magenta(),
    ]);

    let header = Paragraph::new(line).block(Block::bordered().title("Session"));
    frame.render_widget(header, area);
}

fn render_benchmarks(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let items = app
        .benchmarks
        .iter()
        .map(|b| {
            let run = if b.run_options.is_empty() {
                "-".to_string()
            } else {
                b.run_options.join(" ")
            };
            ListItem::new(format!(
                "{} | {} [{}] ({run})",
                b.name, b.category, b.data_type
            ))
        })
        .collect::<Vec<_>>();

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

fn render_ir_diff_steps(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No run result yet.\nPress 'a' to build+run.")
                .block(Block::bordered().title("IR Steps")),
            area,
        );
        return;
    };

    if session.ir_diff_steps.is_empty() {
        let run_hint = if let Some(first) = session.loop_results.first() {
            format!(
                "Latest run: {} {:.2}s checksum={}",
                first.loop_id, first.time_sec, first.checksum
            )
        } else {
            String::from("No run rows captured yet.")
        };
        frame.render_widget(
            Paragraph::new(format!("No IR diff captured in latest build.\n{run_hint}"))
                .block(Block::bordered().title("IR Steps")),
            area,
        );
        return;
    }

    let items = session
        .ir_diff_steps
        .iter()
        .map(|step| {
            ListItem::new(format!(
                "#{:03} {} @ {} [Δ{} | R:{}]",
                step.index,
                step.pass,
                step.target,
                step.changed_lines,
                step.remark_indices.len()
            ))
        })
        .collect::<Vec<_>>();

    let steps_title = if app.is_steps_focused() {
        "IR Steps [Focus]"
    } else {
        "IR Steps"
    };
    let list = List::new(items)
        .block(Block::bordered().title(steps_title))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(Some(app.selected_step_index()));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_ir_diff(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No session available.").block(Block::bordered().title("IR Diff")),
            area,
        );
        return;
    };

    let diff_title = if app.is_ir_diff_focused() {
        "IR Diff [Focus]"
    } else {
        "IR Diff"
    };
    let mut lines = vec![Line::from(format!("Status: {}", session.status))];
    lines.push(Line::from(format!(
        "Overlay: {} (press 'o' to toggle)",
        if app.overlay_enabled { "ON" } else { "OFF" }
    )));
    lines.push(Line::from(format!("Scroll: {}", app.diff_scroll)));
    if let Some(selected_loop_id) = app.selected_function_loop_id() {
        lines.push(Line::from(format!("Selected loop: {selected_loop_id}")));
    }
    if let Some(selected_symbol) = app.selected_function_symbol() {
        lines.push(Line::from(format!("Selected symbol: {selected_symbol}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Run rows:"));
    if session.loop_results.is_empty() {
        lines.push(Line::from("- no rows captured"));
    } else {
        for row in session.loop_results.iter().take(10) {
            lines.push(Line::from(format!(
                "- {} {:.2}s checksum={}",
                row.loop_id, row.time_sec, row.checksum
            )));
        }
        if session.loop_results.len() > 10 {
            lines.push(Line::from(format!(
                "... {} more run rows omitted",
                session.loop_results.len() - 10
            )));
        }
    }

    let Some(step) = app.selected_ir_diff_step() else {
        lines.push(Line::from(""));
        lines.push(Line::from(
            "No IR diff captured in latest build (incremental/no source change).",
        ));
        let paragraph = Paragraph::new(Text::from(lines))
            .block(Block::bordered().title(diff_title))
            .scroll((app.diff_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
        return;
    };

    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "Step #{:03} | Pass: {} | Target: {} | changed_lines={}",
        step.index, step.pass, step.target, step.changed_lines
    )));
    if let Some(first_remark_step) = session.optimization_steps.first() {
        lines.push(Line::from(format!(
            "Remark steps: {} (first pass: {})",
            session.optimization_steps.len(),
            first_remark_step.pass
        )));
    }
    lines.push(Line::from(""));

    if app.overlay_enabled {
        if step.remark_indices.is_empty() {
            lines.push(Line::from("Overlay: no matched remarks"));
        } else {
            for remark_idx in step.remark_indices.iter().take(20) {
                let Some(r) = session.remarks.get(*remark_idx) else {
                    continue;
                };
                let location = match (&r.file, r.line) {
                    (Some(file), Some(line)) => format!("{file}:{line}"),
                    (Some(file), None) => file.clone(),
                    _ => String::from("-"),
                };
                let function = r.function.as_deref().unwrap_or("-");
                let message = r.message.as_deref().unwrap_or("-");
                lines.push(Line::from(format!(
                    "[{}] {} @ {} ({}) {}",
                    r.kind, r.name, location, function, message
                )));
            }
            if step.remark_indices.len() > 20 {
                lines.push(Line::from(format!(
                    "... {} more remarks omitted",
                    step.remark_indices.len() - 20
                )));
            }
        }
        lines.push(Line::from(""));
    }

    if step.diff_text.trim().is_empty() {
        lines.push(Line::from("No diff text available for this step."));
    } else {
        lines.extend(
            step.diff_text
                .lines()
                .map(|line| Line::from(line.to_string())),
        );
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title(diff_title))
        .scroll((app.diff_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_list_footer(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let hints = "q quit | left/right focus pane | up/down select-or-scroll | enter select function";
    let text = Text::from(vec![
        Line::from(hints),
        Line::from(format!("Status: {}", app.status_message)),
    ]);

    let footer = Paragraph::new(text).block(Block::bordered().title("Keys"));
    frame.render_widget(footer, area);
}

fn render_detail_footer(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let hints = "q quit | esc back | left/right focus tab | up/down step-or-scroll | p profile | b build | r run | a build+run | o overlay | c clear";
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

    let hint = Paragraph::new("up/down move | enter confirm | esc cancel")
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
