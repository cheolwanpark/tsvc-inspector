mod app;
mod bootstrap;
mod discovery;
mod error;
mod input;
mod model;
mod parser;
mod runner;
mod ui;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use anyhow::{Context, anyhow};
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};

use crate::app::{AppState, JobEvent, JobOutcome};
use crate::error::AppResult;
use crate::input::UserAction;
use crate::model::{AppPage, JobKind, RemarksSummary};
use crate::runner::RunnerConfig;

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

    #[arg(long, default_value = ".")]
    build_root: PathBuf,

    #[arg(long, default_value_t = default_jobs())]
    jobs: usize,
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
        build_root: cli.build_root,
        jobs: cli.jobs,
    };

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

    let mut app = AppState::new(benchmarks);
    let (job_tx, job_rx) = mpsc::channel::<JobEvent>();

    ratatui::run(move |terminal| run_app(terminal, &mut app, &runner_config, &job_tx, &job_rx))
        .map_err(|e| anyhow!("terminal run failed: {e}"))?;

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
                action => match app.page {
                    AppPage::BenchmarkList => match action {
                        UserAction::MoveUp => app.select_prev(),
                        UserAction::MoveDown => app.select_next(),
                        UserAction::OpenBenchmarkPage => app.open_selected_benchmark_page(),
                        UserAction::Build | UserAction::Run | UserAction::BuildAndRun => {
                            app.set_status_message("Open benchmark page with Enter to run jobs");
                        }
                        _ => {}
                    },
                    AppPage::BenchmarkDetail => match action {
                        UserAction::MoveUp => app.detail_move_up(),
                        UserAction::MoveDown => app.detail_move_down(),
                        UserAction::BackToBenchmarkList => app.back_to_benchmark_list(),
                        UserAction::FocusPrevTab => app.focus_prev_tab(),
                        UserAction::FocusNextTab => app.focus_next_tab(),
                        UserAction::CycleProfile => app.cycle_profile(),
                        UserAction::ToggleOverlay => app.toggle_overlay(),
                        UserAction::ClearSession => app.clear_session(),
                        UserAction::Build => {
                            maybe_spawn_job(app, config, job_tx, JobKind::Build);
                        }
                        UserAction::Run => {
                            maybe_spawn_job(app, config, job_tx, JobKind::Run);
                        }
                        UserAction::BuildAndRun => {
                            maybe_spawn_job(app, config, job_tx, JobKind::BuildAndRun);
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

    let profile = app.active_profile;
    let tx = job_tx.clone();
    let cfg = config.clone();
    app.begin_job(kind, benchmark.name.clone(), profile);

    std::thread::spawn(move || {
        let _ = tx.send(JobEvent::Started {
            kind,
            benchmark: benchmark.name.clone(),
            profile,
        });

        let exec_result = runner::execute_job(&cfg, &benchmark, profile, kind, |line| {
            let _ = tx.send(JobEvent::LogLine(line));
        });

        match exec_result {
            Ok(raw) => {
                let loop_results = parser::parse_tsvc_stdout(&raw.run_stdout);
                let remarks = if let Some(path) = raw.remark_file {
                    match parser::parse_opt_remarks(&path) {
                        Ok(entries) => entries,
                        Err(err) => {
                            let _ =
                                tx.send(JobEvent::LogLine(format!("remark parse warning: {err}")));
                            Vec::new()
                        }
                    }
                } else {
                    Vec::new()
                };

                let optimization_steps = parser::group_optimization_steps(&remarks);
                let ir_diff_steps = parser::parse_ir_diff_steps(&raw.build_trace, &remarks);
                let summary = RemarksSummary::from_entries(&remarks);
                let outcome = JobOutcome {
                    benchmark: benchmark.name,
                    profile,
                    loop_results,
                    remarks,
                    ir_diff_steps,
                    optimization_steps,
                    remarks_summary: summary,
                };
                let _ = tx.send(JobEvent::Finished(Ok(outcome)));
            }
            Err(err) => {
                let _ = tx.send(JobEvent::Finished(Err(format!("{err:#}"))));
            }
        }
    });
}
