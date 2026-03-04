mod app;
mod bootstrap;
mod discovery;
mod error;
mod input;
mod model;
mod parser;
mod runner;
mod tsvc_patch;
mod ui;

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use anyhow::{Context, anyhow};
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};

use crate::app::{AppState, JobEvent, JobOutcome, JobOutcomeData};
use crate::error::AppResult;
use crate::input::UserAction;
use crate::model::{
    AppPage, BenchmarkFunction, BuildPurpose, CompileProfile, FunctionRunMode, JobKind,
    LoopResult, RemarkEntry, RemarksSummary,
};
use crate::runner::RunnerConfig;
use crate::tsvc_patch::TsvcPatchOutcome;

#[derive(Parser, Debug)]
#[command(
    name = "tsvc-tui",
    version,
    about = "TSVC study assistant with Ratatui"
)]
struct Cli {
    #[arg(long, default_value = "llvm-test-suite")]
    tsvc_root: PathBuf,

    #[arg(long, default_value = "clang")]
    clang: String,

    #[arg(long, default_value = "cmake")]
    cmake: String,

    #[arg(long, default_value = "opt")]
    opt: String,

    #[arg(long, default_value = ".")]
    build_root: PathBuf,

    #[arg(long, default_value_t = default_jobs())]
    jobs: usize,

    #[arg(long, default_value_t = 80)]
    analysis_window: usize,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn main() -> AppResult<()> {
    let cli = Cli::parse();
    let resolved_tsvc_root = bootstrap::resolve_tsvc_root(&cli.tsvc_root).with_context(|| {
        format!(
            "failed to resolve TSVC root from {}",
            cli.tsvc_root.display()
        )
    })?;

    let runner_config = RunnerConfig {
        tsvc_root: resolved_tsvc_root.clone(),
        clang: cli.clang,
        cmake: cli.cmake,
        opt: cli.opt,
        build_root: cli.build_root,
        jobs: cli.jobs,
        analysis_window: cli.analysis_window,
    };

    let (function_run_mode, startup_status) = configure_function_run_mode(&runner_config)?;

    let benchmarks = discovery::discover_benchmarks(&resolved_tsvc_root).with_context(|| {
        format!(
            "failed to discover TSVC benchmarks under {}",
            resolved_tsvc_root.display()
        )
    })?;
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

fn configure_function_run_mode(
    config: &RunnerConfig,
) -> AppResult<(FunctionRunMode, Option<String>)> {
    if !bootstrap::is_app_managed_fallback_root(&config.tsvc_root) {
        return Ok((
            FunctionRunMode::OutputFilter,
            Some(String::from(
                "External TSVC root detected: function mode = output-filter",
            )),
        ));
    }

    match tsvc_patch::ensure_function_filter_patch(&config.tsvc_root) {
        Ok(TsvcPatchOutcome::AlreadyPatched) => Ok((
            FunctionRunMode::RealSelective,
            Some(String::from("Function mode: real-selective")),
        )),
        Ok(TsvcPatchOutcome::Patched) => {
            if let Err(err) = reset_profile_build_dirs(config) {
                eprintln!("warning: patched TSVC source but failed to reset build dirs: {err:#}");
            }
            Ok((
                FunctionRunMode::RealSelective,
                Some(String::from(
                    "Patched fallback TSVC for function-selective run mode",
                )),
            ))
        }
        Err(err) => {
            eprintln!("warning: failed to patch TSVC for real-selective mode: {err:#}");
            Ok((
                FunctionRunMode::OutputFilter,
                Some(String::from(
                    "Function patch failed: falling back to output-filter mode",
                )),
            ))
        }
    }
}

fn reset_profile_build_dirs(config: &RunnerConfig) -> AppResult<()> {
    for profile in [
        CompileProfile::O3Remarks,
        CompileProfile::O3NoVec,
        CompileProfile::O3Default,
    ] {
        for purpose in [BuildPurpose::Runtime, BuildPurpose::Analysis] {
            let dir = config.build_root.join(profile.build_dir_name_for(purpose));
            if dir.exists() {
                fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
            }
        }
    }
    Ok(())
}

fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut AppState,
    config: &RunnerConfig,
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

            match input::map_key_event(key) {
                UserAction::Quit => break Ok(()),
                UserAction::None => {}
                action if app.is_function_modal_open() => match action {
                    UserAction::MoveUp => app.function_modal_move_up(),
                    UserAction::MoveDown => app.function_modal_move_down(),
                    UserAction::Confirm => app.confirm_function_selection(),
                    UserAction::BackToBenchmarkList => app.close_function_select_modal(),
                    _ => {}
                },
                action => match app.page {
                    AppPage::BenchmarkList => match action {
                        UserAction::MoveUp => app.list_move_up(),
                        UserAction::MoveDown => app.list_move_down(),
                        UserAction::FocusNextPaneCycle => app.focus_next_list_pane(),
                        UserAction::FocusPrevPaneCycle => app.focus_prev_list_pane(),
                        UserAction::Confirm => app.open_function_select_modal(),
                        UserAction::Run | UserAction::Analyze => {
                            app.set_status_message(
                                "Select function and open detail page to run or analyze",
                            );
                        }
                        _ => {}
                    },
                    AppPage::BenchmarkDetail => match action {
                        UserAction::MoveUp => app.detail_move_up(),
                        UserAction::MoveDown => app.detail_move_down(),
                        UserAction::BackToBenchmarkList => {
                            if app.is_ir_view_focused() {
                                app.detail_focus = crate::app::DetailFocus::PassList;
                            } else if app.is_pass_focused() {
                                app.detail_focus = crate::app::DetailFocus::StageList;
                            } else if app.is_source_view_focused() {
                                app.detail_focus = crate::app::DetailFocus::StageList;
                            } else {
                                app.back_to_benchmark_list();
                            }
                        }
                        UserAction::FocusNextPaneCycle => app.focus_cycle_next(),
                        UserAction::FocusPrevPaneCycle => app.focus_cycle_prev(),
                        UserAction::Confirm => {
                            if app.is_stage_focused() {
                                app.detail_focus = crate::app::DetailFocus::PassList;
                            } else if app.is_pass_focused() {
                                app.detail_focus = crate::app::DetailFocus::IrView;
                            }
                        }
                        UserAction::CycleProfile => app.cycle_profile(),
                        UserAction::ClearSession => app.clear_session(),
                        UserAction::Run => {
                            maybe_spawn_job(app, config, job_tx, JobKind::BuildAndRun);
                        }
                        UserAction::Analyze => {
                            maybe_spawn_job(app, config, job_tx, JobKind::AnalyzeFast);
                        }
                        _ => {}
                    },
                },
            }
        }
    }
}

fn maybe_spawn_job(
    app: &mut AppState,
    config: &RunnerConfig,
    job_tx: &Sender<JobEvent>,
    kind: JobKind,
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

    let profile = app.active_profile;
    let run_mode = app.function_run_mode;
    let tx = job_tx.clone();
    let cfg = config.clone();

    std::thread::spawn(move || {
        let _ = tx.send(JobEvent::Started {
            kind,
            benchmark: benchmark.name.clone(),
            profile,
            selected_function: selected_function.clone(),
            run_mode,
        });

        match kind {
            JobKind::BuildAndRun => {
                let exec_result = runner::execute_runtime_job(
                    &cfg,
                    &benchmark,
                    profile,
                    &selected_function.symbol,
                    run_mode,
                    |line| {
                        let _ = tx.send(JobEvent::LogLine(line));
                    },
                );
                match exec_result {
                    Ok(raw) => {
                        let parsed_loop_results = parser::parse_tsvc_stdout(&raw.run_stdout);
                        let parsed_remarks = parse_remarks_with_log(raw.remark_file, &tx);
                        let loop_results = filter_loop_results_for_selected_function(
                            parsed_loop_results,
                            &selected_function,
                        );
                        let remarks = filter_remarks_for_selected_function(
                            parsed_remarks,
                            &selected_function,
                        );
                        if loop_results.is_empty() {
                            let _ = tx.send(JobEvent::Finished(Err(format!(
                                "selected function '{}' was not found in run output",
                                selected_function.loop_id
                            ))));
                            return;
                        }

                        let summary = RemarksSummary::from_entries(&remarks);
                        let outcome = JobOutcome {
                            kind,
                            benchmark: benchmark.name.clone(),
                            profile,
                            selected_function: selected_function.clone(),
                            run_mode,
                            data: JobOutcomeData::Runtime {
                                loop_results,
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
            }
            JobKind::AnalyzeFast => {
                let exec_result =
                    runner::execute_analysis_fast(&cfg, &benchmark, profile, |line| {
                        let _ = tx.send(JobEvent::LogLine(line));
                    });
                match exec_result {
                    Ok(raw) => {
                        let parsed_remarks = parse_remarks_with_log(raw.remark_file, &tx);
                        let remarks = filter_remarks_for_selected_function(
                            parsed_remarks,
                            &selected_function,
                        );
                        let analysis_steps = parser::build_fast_analysis_steps(
                            &raw.build_trace,
                            &selected_function.symbol,
                            &remarks,
                        );
                        if analysis_steps.is_empty() {
                            let _ = tx.send(JobEvent::Finished(Err(format!(
                                "no function-level IR steps found for '{}'",
                                selected_function.symbol
                            ))));
                            return;
                        }

                        let summary = RemarksSummary::from_entries(&remarks);
                        let outcome = JobOutcome {
                            kind,
                            benchmark: benchmark.name.clone(),
                            profile,
                            selected_function: selected_function.clone(),
                            run_mode,
                            data: JobOutcomeData::Analysis {
                                analysis_steps,
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

fn filter_loop_results_for_selected_function(
    loop_results: Vec<LoopResult>,
    selected_function: &BenchmarkFunction,
) -> Vec<LoopResult> {
    loop_results
        .into_iter()
        .filter(|entry| {
            entry
                .loop_id
                .eq_ignore_ascii_case(&selected_function.loop_id)
        })
        .collect::<Vec<_>>()
}

fn filter_remarks_for_selected_function(
    remarks: Vec<RemarkEntry>,
    selected_function: &BenchmarkFunction,
) -> Vec<RemarkEntry> {
    remarks
        .into_iter()
        .filter(|remark| {
            remark
                .function
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case(&selected_function.symbol))
        })
        .collect::<Vec<_>>()
}
