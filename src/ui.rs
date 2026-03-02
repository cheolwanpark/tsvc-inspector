use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Row, Table, Tabs, Wrap};

use crate::app::{AppState, JobState};
use crate::model::RightTab;

pub fn render(frame: &mut Frame, app: &AppState) {
    let vertical = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ]);
    let [header_area, main_area, footer_area] = vertical.areas(frame.area());
    render_header(frame, app, header_area);

    let horizontal = Layout::horizontal([
        Constraint::Percentage(28),
        Constraint::Percentage(34),
        Constraint::Percentage(38),
    ]);
    let [left_area, center_area, right_area] = horizontal.areas(main_area);

    render_benchmarks(frame, app, left_area);
    render_loop_results(frame, app, center_area);
    render_inspector(frame, app, right_area);
    render_footer(frame, app, footer_area);
}

fn render_header(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
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
        "Benchmark: ".gray(),
        selected.yellow(),
        "  Profile: ".gray(),
        app.active_profile.to_string().green(),
        "  Job: ".gray(),
        job.cyan(),
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
            ListItem::new(format!("{} [{}] ({run})", b.category, b.data_type))
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

fn render_loop_results(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(session) = &app.current_session else {
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

    let tabs = Tabs::new(vec!["Remarks", "Build Log"])
        .block(Block::bordered().title("Inspector"))
        .select(app.active_tab.index())
        .highlight_style(Style::default().fg(Color::Yellow));
    frame.render_widget(tabs, tabs_area);

    match app.active_tab {
        RightTab::Remarks => render_remarks(frame, app, content_area),
        RightTab::BuildLog => render_build_log(frame, app, content_area),
    }
}

fn render_remarks(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(session) = &app.current_session else {
        frame.render_widget(
            Paragraph::new("No session available.").block(Block::bordered().title("Remarks")),
            area,
        );
        return;
    };

    let mut lines = Vec::new();
    lines.push(Line::from(format!(
        "Summary: total={} vectorized={} missed={} not_beneficial={}",
        session.remarks_summary.total_loop_vectorize,
        session.remarks_summary.vectorized,
        session.remarks_summary.missed_details,
        session.remarks_summary.not_beneficial
    )));
    lines.push(Line::from(format!("Status: {}", session.status)));
    lines.push(Line::from(""));

    if session.remarks.is_empty() {
        lines.push(Line::from("No loop-vectorize remarks found."));
    } else {
        for r in session.remarks.iter().take(80) {
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
        .block(Block::bordered().title("Remarks"))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_build_log(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let Some(session) = &app.current_session else {
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

fn render_footer(frame: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let hints = "q quit | up/down select | p profile | b build | r run | a build+run | tab switch tab | c clear";
    let text = Text::from(vec![
        Line::from(hints),
        Line::from(format!("Status: {}", app.status_message)),
    ]);

    let footer = Paragraph::new(text).block(Block::bordered().title("Keys"));
    frame.render_widget(footer, area);
}
