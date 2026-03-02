use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Row, Table, Tabs, Wrap};

use crate::app::{AppState, JobState};
use crate::model::{AppPage, RightTab};

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

    let horizontal = Layout::horizontal([
        Constraint::Percentage(26),
        Constraint::Percentage(34),
        Constraint::Percentage(40),
    ]);
    let [steps_area, loop_area, inspector_area] = horizontal.areas(main_area);
    render_optimization_steps(frame, app, steps_area);
    render_loop_results(frame, app, loop_area);
    render_inspector(frame, app, inspector_area);
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

fn render_optimization_steps(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No run result yet.\nPress 'a' to build+run.")
                .block(Block::bordered().title("Optimization Steps")),
            area,
        );
        return;
    };

    if session.optimization_steps.is_empty() {
        frame.render_widget(
            Paragraph::new("No optimization remarks found.")
                .block(Block::bordered().title("Optimization Steps")),
            area,
        );
        return;
    }

    let items = session
        .optimization_steps
        .iter()
        .map(|step| {
            ListItem::new(format!(
                "{} [{} | P:{} M:{} A:{} O:{}]",
                step.pass, step.total, step.passed, step.missed, step.analysis, step.other
            ))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(Block::bordered().title("Optimization Steps"))
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

fn render_loop_results(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No run result yet.\nPress 'a' to build+run.")
                .block(Block::bordered().title("Loop Results")),
            area,
        );
        return;
    };

    if session.loop_results.is_empty() {
        frame.render_widget(
            Paragraph::new("No loop rows parsed.").block(Block::bordered().title("Loop Results")),
            area,
        );
        return;
    }

    let rows = session
        .loop_results
        .iter()
        .take(300)
        .map(|r| {
            Row::new(vec![
                r.loop_id.clone(),
                format!("{:.2}", r.time_sec),
                r.checksum.clone(),
            ])
        })
        .collect::<Vec<_>>();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(12),
        ],
    )
    .header(
        Row::new(vec!["Loop", "Time(s)", "Checksum"]).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::bordered().title("Loop Results"));
    frame.render_widget(table, area);
}

fn render_inspector(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let vertical = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]);
    let [tabs_area, content_area] = vertical.areas(area);

    let tabs = Tabs::new(vec!["Step Details", "Build Log"])
        .block(Block::bordered().title("Inspector"))
        .select(app.active_tab.index())
        .highlight_style(Style::default().fg(Color::Yellow));
    frame.render_widget(tabs, tabs_area);

    match app.active_tab {
        RightTab::StepDetails => render_step_details(frame, app, content_area),
        RightTab::BuildLog => render_build_log(frame, app, content_area),
    }
}

fn render_step_details(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No session available.").block(Block::bordered().title("Step Details")),
            area,
        );
        return;
    };

    let Some(step) = app.selected_optimization_step() else {
        frame.render_widget(
            Paragraph::new("No optimization steps available.")
                .block(Block::bordered().title("Step Details")),
            area,
        );
        return;
    };

    let mut lines = Vec::new();
    lines.push(Line::from(format!(
        "Pass: {} | total={} passed={} missed={} analysis={} other={}",
        step.pass, step.total, step.passed, step.missed, step.analysis, step.other
    )));
    lines.push(Line::from(format!("Status: {}", session.status)));
    lines.push(Line::from(""));

    if step.remark_indices.is_empty() {
        lines.push(Line::from("No remarks in this step."));
    } else {
        for remark_idx in step.remark_indices.iter().take(120) {
            let Some(r) = session.remarks.get(*remark_idx) else {
                continue;
            };
            let mut location = String::from("-");
            if let Some(file) = &r.file {
                location = match r.line {
                    Some(line) => format!("{file}:{line}"),
                    None => file.clone(),
                };
            }
            let function = r.function.as_deref().unwrap_or("-");
            let message = r.message.as_deref().unwrap_or("-");
            lines.push(Line::from(format!(
                "[{}] {} @ {} ({}) {}",
                r.kind, r.name, location, function, message
            )));
        }
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::bordered().title("Step Details"))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_build_log(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(session) = app.active_session_for_selected_benchmark() else {
        frame.render_widget(
            Paragraph::new("No session available.").block(Block::bordered().title("Build Log")),
            area,
        );
        return;
    };

    let tail = session
        .logs
        .iter()
        .rev()
        .take(250)
        .cloned()
        .collect::<Vec<_>>();
    let text = tail.into_iter().rev().collect::<Vec<_>>().join("\n");

    let paragraph = Paragraph::new(text)
        .block(Block::bordered().title("Build Log"))
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
    let hints = "q quit | esc back | left/right step | p profile | b build | r run | a build+run | tab switch tab | c clear";
    let text = Text::from(vec![
        Line::from(hints),
        Line::from(format!("Status: {}", app.status_message)),
    ]);

    let footer = Paragraph::new(text).block(Block::bordered().title("Keys"));
    frame.render_widget(footer, area);
}
