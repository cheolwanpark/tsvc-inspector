use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use anyhow::{Context, anyhow};
use crossterm::event::{self, Event, KeyEventKind};

use crate::core::error::AppResult;
use crate::core::model::{AppPage, JobKind, RemarkEntry, RemarksSummary};
use crate::data::{discovery, parser, repo, runner};
use crate::display::app::{AppState, JobEvent, JobOutcome, JobOutcomeData};
use crate::display::clipboard;
use crate::display::input::{self, UserAction};
use crate::display::ui;
use crate::transform::{analysis, catalog, filtering};

#[derive(Debug, Clone)]
pub struct RuntimeOptions {
    pub tsvc_root: PathBuf,
    pub clang: String,
    pub build_root: PathBuf,
    pub jobs: usize,
}

pub fn run(options: RuntimeOptions) -> AppResult<()> {
    let resolved_tsvc_root = repo::resolve_tsvc_root(&options.tsvc_root).with_context(|| {
        format!(
            "failed to resolve TSVC root from {}",
            options.tsvc_root.display()
        )
    })?;

    let runner_config = runner::RunnerConfig {
        tsvc_root: resolved_tsvc_root.clone(),
        clang: options.clang,
        build_root: options.build_root,
        jobs: options.jobs,
    };

    let (function_run_mode, startup_status) = repo::configure_function_run_mode(&runner_config)?;

    let raw_benchmarks =
        discovery::discover_raw_benchmarks(&resolved_tsvc_root).with_context(|| {
            format!(
                "failed to discover TSVC benchmarks under {}",
                resolved_tsvc_root.display()
            )
        })?;
    let benchmarks = catalog::build_benchmark_catalog(raw_benchmarks);
    if benchmarks.is_empty() {
        return Err(anyhow!(
            "no TSVC benchmarks found under {}/MultiSource/Benchmarks/TSVC",
            resolved_tsvc_root.display()
        ));
    }

    let mut app = AppState::new_with_run_mode(benchmarks, function_run_mode);
    if let Some(msg) = startup_status {
        app.set_status_message(msg);
    }

    let (job_tx, job_rx) = mpsc::channel::<JobEvent>();

    ratatui::run(move |terminal| run_app(terminal, &mut app, &runner_config, &job_tx, &job_rx))
        .map_err(|e| anyhow!("terminal run failed: {e}"))?;

    Ok(())
}

fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut AppState,
    config: &runner::RunnerConfig,
    job_tx: &Sender<JobEvent>,
    job_rx: &Receiver<JobEvent>,
) -> std::io::Result<()> {
    loop {
        while let Ok(job_event) = job_rx.try_recv() {
            app.handle_job_event(job_event);
        }

        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            if app.page == AppPage::BenchmarkList
                && app.is_config_modal_open()
                && app.is_config_text_editing()
            {
                match key.code {
                    crossterm::event::KeyCode::Esc => app.cancel_config_text_edit(),
                    crossterm::event::KeyCode::Enter => app.config_confirm(),
                    crossterm::event::KeyCode::Backspace => app.config_backspace(),
                    crossterm::event::KeyCode::Char(ch) => app.config_push_char(ch),
                    _ => {}
                }
                continue;
            }

            match input::map_key_event(key) {
                UserAction::Quit => break Ok(()),
                UserAction::None => {}
                action if app.is_function_modal_open() => match action {
                    UserAction::MoveUp => app.function_modal_move_up(),
                    UserAction::MoveDown => app.function_modal_move_down(),
                    UserAction::Confirm => {
                        let entered_detail = app.confirm_function_selection();
                        if entered_detail {
                            maybe_spawn_analysis(app, config, job_tx);
                        }
                    }
                    UserAction::BackToBenchmarkList => app.close_function_select_modal(),
                    _ => {}
                },
                action if app.page == AppPage::BenchmarkDetail && app.is_pass_info_modal_open() => {
                    match action {
                        UserAction::MoveUp => app.detail_move_up(),
                        UserAction::MoveDown => app.detail_move_down(),
                        UserAction::BackToBenchmarkList => {
                            app.close_pass_info_modal();
                        }
                        _ => {}
                    }
                }
                action if app.page == AppPage::BenchmarkDetail && app.is_c_source_popup_open() => {
                    match action {
                        UserAction::MoveUp => app.detail_move_up(),
                        UserAction::MoveDown => app.detail_move_down(),
                        UserAction::BackToBenchmarkList | UserAction::ShowCSource => {
                            app.close_c_source_popup();
                        }
                        _ => {}
                    }
                }
                action
                    if app.page == AppPage::BenchmarkDetail && app.is_side_by_side_diff_open() =>
                {
                    match action {
                        UserAction::MoveUp => app.side_by_side_diff_scroll_up(),
                        UserAction::MoveDown => app.side_by_side_diff_scroll_down(),
                        UserAction::BackToBenchmarkList | UserAction::ToggleSideBySideDiff => {
                            app.close_side_by_side_diff();
                            app.set_status_message("Side-by-side diff closed");
                        }
                        _ => {}
                    }
                }
                action if app.page == AppPage::BenchmarkList && app.is_config_modal_open() => {
                    match action {
                        UserAction::MoveUp => app.config_move_up(),
                        UserAction::MoveDown => app.config_move_down(),
                        UserAction::Confirm => app.config_confirm(),
                        UserAction::BackToBenchmarkList => app.close_config_modal(),
                        UserAction::Backspace => app.config_backspace(),
                        UserAction::TextChar(ch) => app.config_push_char(ch),
                        _ => {}
                    }
                }
                action => match app.page {
                    AppPage::BenchmarkList => match action {
                        UserAction::MoveUp => app.list_move_up(),
                        UserAction::MoveDown => app.list_move_down(),
                        UserAction::MoveLeft => app.list_move_left(),
                        UserAction::MoveRight => app.list_move_right(),
                        UserAction::Confirm => app.open_function_select_modal(),
                        UserAction::ShowCSource => app.open_config_modal(),
                        _ => {}
                    },
                    AppPage::BenchmarkDetail => match action {
                        UserAction::MoveUp => app.detail_move_up(),
                        UserAction::MoveDown => app.detail_move_down(),
                        UserAction::MoveLeft => app.detail_move_left(),
                        UserAction::MoveRight => app.detail_move_right(),
                        UserAction::BackToBenchmarkList => app.back_to_benchmark_list(),
                        UserAction::Confirm => {
                            if app.is_selector_focused() {
                                app.open_pass_info_modal();
                            }
                        }
                        UserAction::RotateCodeViewMode => {
                            if app.is_code_view_focused() {
                                app.rotate_code_view_mode_next();
                            }
                        }
                        UserAction::RotateCodeViewModePrev => {
                            if app.is_code_view_focused() {
                                app.rotate_code_view_mode_prev();
                            }
                        }
                        UserAction::CyclePassTimelineFilter => app.cycle_pass_timeline_filter(),
                        UserAction::ShowCSource => app.open_c_source_popup(),
                        UserAction::CopyDetailToClipboard => copy_detail_snapshot(app),
                        UserAction::ToggleSideBySideDiff => app.toggle_side_by_side_diff(),
                        _ => {}
                    },
                },
            }
        }
    }
}

fn copy_detail_snapshot(app: &mut AppState) {
    let payload = match app.build_detail_copy_payload() {
        Ok(payload) => payload,
        Err(err) => {
            app.set_status_message(format!("Nothing to copy: {err}"));
            return;
        }
    };

    match clipboard::copy_text(&payload) {
        Ok(()) => app.set_status_message("Copied detail snapshot to clipboard"),
        Err(err) => app.set_status_message(format!("Copy failed: {err}")),
    }
}

fn maybe_spawn_analysis(
    app: &mut AppState,
    config: &runner::RunnerConfig,
    job_tx: &Sender<JobEvent>,
) {
    if app.is_job_running() {
        app.set_status_message("A job is already running");
        return;
    }

    let Some(benchmark) = app.selected_benchmark().cloned() else {
        app.set_status_message("No benchmark selected");
        return;
    };
    let Some(selected_function) = app.selected_function_for_selected_benchmark().cloned() else {
        app.set_status_message("Select a function first");
        return;
    };

    let compiler_config = app.current_compiler_config();
    let run_mode = app.function_run_mode;
    let tx = job_tx.clone();
    let cfg = config.clone();

    std::thread::spawn(move || {
        let _ = tx.send(JobEvent::Started {
            kind: JobKind::AnalyzeFast,
            benchmark: benchmark.name.clone(),
            compiler_config: compiler_config.clone(),
            selected_function: selected_function.clone(),
            run_mode,
        });

        let exec_result = runner::execute_analysis_fast(
            &cfg,
            &benchmark,
            &compiler_config,
            &selected_function.symbol,
            |line| {
                let _ = tx.send(JobEvent::LogLine(line));
            },
        );
        match exec_result {
            Ok(raw) => {
                let parsed_remarks = parse_remarks_with_log(raw.remark_file, &tx);
                let remarks = filtering::filter_remarks_for_selected_function(
                    parsed_remarks,
                    &selected_function,
                );
                let analysis_steps = analysis::build_fast_analysis_steps(
                    &raw.build_trace,
                    &selected_function.symbol,
                    &remarks,
                    raw.source_file_content.as_deref(),
                );
                let pass_trace = analysis::build_pass_trace(
                    &raw.build_trace,
                    &selected_function.symbol,
                    &analysis_steps,
                    &remarks,
                );
                if pass_trace.is_empty() {
                    let _ = tx.send(JobEvent::Finished(Err(format!(
                        "no function-level pass trace found for '{}'",
                        selected_function.symbol
                    ))));
                    return;
                }

                let summary = RemarksSummary::from_entries(&remarks);
                let outcome = JobOutcome {
                    kind: JobKind::AnalyzeFast,
                    benchmark: benchmark.name.clone(),
                    compiler_config: compiler_config.clone(),
                    selected_function: selected_function.clone(),
                    run_mode,
                    data: JobOutcomeData::Analysis {
                        analysis_steps,
                        pass_trace,
                        remarks,
                        remarks_summary: summary,
                    },
                };
                let _ = tx.send(JobEvent::Finished(Ok(outcome)));
            }
            Err(err) => {
                let _ = tx.send(JobEvent::Finished(Err(format!("{err:#}"))));
            }
        }
    });
}

fn parse_remarks_with_log(path: Option<PathBuf>, tx: &Sender<JobEvent>) -> Vec<RemarkEntry> {
    let Some(path) = path else {
        return Vec::new();
    };
    match parser::parse_opt_remarks(&path) {
        Ok(entries) => entries,
        Err(err) => {
            let _ = tx.send(JobEvent::LogLine(format!("remark parse warning: {err}")));
            Vec::new()
        }
    }
}
