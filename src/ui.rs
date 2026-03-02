use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Wrap};

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

    let horizontal = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]);
    let [list_area, info_area] = horizontal.areas(main_area);
    render_benchmarks(frame, app, list_area);
    render_benchmark_info(frame, app, info_area);
    render_list_footer(frame, app, footer_area);
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
        "  Profile: ".gray(),
        app.active_profile.to_string().cyan(),
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

    let list = List::new(items)
        .block(Block::bordered().title("Benchmarks"))
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

fn render_benchmark_info(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(benchmark) = app.selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No benchmark selected.").block(Block::bordered().title("Info")),
            area,
        );
        return;
    };

    let run_options = if benchmark.run_options.is_empty() {
        String::from("-")
    } else {
        benchmark.run_options.join(" ")
    };
    let lines = vec![
        Line::from(format!("Name: {}", benchmark.name)),
        Line::from(format!("Category: {}", benchmark.category)),
        Line::from(format!("Data Type: {}", benchmark.data_type)),
        Line::from(format!("Run Options: {run_options}")),
        Line::from(""),
        Line::from("Press Enter to open benchmark detail page."),
    ];
    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title("Info"))
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
        frame.render_widget(
            Paragraph::new("No IR diff captured.\nBuild with 'b' or 'a'.")
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
    let Some(step) = app.selected_ir_diff_step() else {
        frame.render_widget(
            Paragraph::new("No IR diff captured. Build this benchmark first.")
                .block(Block::bordered().title("IR Diff")),
            area,
        );
        return;
    };

    let diff_title = if app.is_ir_diff_focused() {
        "IR Diff [Focus]"
    } else {
        "IR Diff"
    };
    let mut lines = vec![
        Line::from(format!(
            "Step #{:03} | Pass: {} | Target: {} | changed_lines={}",
            step.index, step.pass, step.target, step.changed_lines
        )),
        Line::from(format!("Status: {}", session.status)),
        Line::from(format!(
            "Overlay: {} (press 'o' to toggle)",
            if app.overlay_enabled { "ON" } else { "OFF" }
        )),
        Line::from(format!("Scroll: {}", app.diff_scroll)),
    ];
    if let Some(first_loop) = session.loop_results.first() {
        lines.push(Line::from(format!(
            "Run sample: {} {:.2}s checksum={}",
            first_loop.loop_id, first_loop.time_sec, first_loop.checksum
        )));
    }
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
    let hints = "q quit | up/down select | enter open benchmark page";
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
