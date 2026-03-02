use std::collections::HashMap;

use crate::model::{
    AppPage, BenchmarkItem, CompileProfile, IrDiffStep, JobKind, LoopResult, OptimizationStep,
    RemarkEntry, RemarksSummary, RunSession, SessionStatus,
};

#[derive(Debug)]
pub enum JobEvent {
    Started {
        kind: JobKind,
        benchmark: String,
        profile: CompileProfile,
    },
    LogLine(String),
    Finished(Result<JobOutcome, String>),
}

#[derive(Debug)]
pub struct JobOutcome {
    pub benchmark: String,
    pub profile: CompileProfile,
    pub loop_results: Vec<LoopResult>,
    pub remarks: Vec<RemarkEntry>,
    pub ir_diff_steps: Vec<IrDiffStep>,
    pub optimization_steps: Vec<OptimizationStep>,
    pub remarks_summary: RemarksSummary,
}

#[derive(Clone, Debug)]
pub enum JobState {
    Idle,
    Running(JobKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailFocus {
    Steps,
    IrDiff,
}

impl DetailFocus {
    fn next(self) -> Self {
        match self {
            Self::Steps => Self::IrDiff,
            Self::IrDiff => Self::Steps,
        }
    }

    fn prev(self) -> Self {
        self.next()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Steps => "Steps",
            Self::IrDiff => "IR Diff",
        }
    }
}

pub struct AppState {
    pub benchmarks: Vec<BenchmarkItem>,
    pub selected_idx: usize,
    pub active_profile: CompileProfile,
    pub overlay_enabled: bool,
    pub page: AppPage,
    pub selected_step_idx: usize,
    pub job_state: JobState,
    pub status_message: String,
    pub detail_focus: DetailFocus,
    pub diff_scroll: u16,
    sessions_by_benchmark: HashMap<String, RunSession>,
    running_benchmark: Option<String>,
}

impl AppState {
    pub fn new(benchmarks: Vec<BenchmarkItem>) -> Self {
        Self {
            benchmarks,
            selected_idx: 0,
            active_profile: CompileProfile::O3Remarks,
            overlay_enabled: false,
            page: AppPage::BenchmarkList,
            selected_step_idx: 0,
            job_state: JobState::Idle,
            status_message: String::from("Ready"),
            detail_focus: DetailFocus::Steps,
            diff_scroll: 0,
            sessions_by_benchmark: HashMap::new(),
            running_benchmark: None,
        }
    }

    pub fn selected_benchmark(&self) -> Option<&BenchmarkItem> {
        self.benchmarks.get(self.selected_idx)
    }

    pub fn select_prev(&mut self) {
        if self.benchmarks.is_empty() {
            return;
        }
        self.selected_idx = self.selected_idx.saturating_sub(1);
    }

    pub fn select_next(&mut self) {
        if self.benchmarks.is_empty() {
            return;
        }
        let max_idx = self.benchmarks.len() - 1;
        self.selected_idx = (self.selected_idx + 1).min(max_idx);
    }

    pub fn cycle_profile(&mut self) {
        self.active_profile = self.active_profile.next();
        self.status_message = format!("Profile: {}", self.active_profile);
    }

    pub fn open_selected_benchmark_page(&mut self) {
        if self.selected_benchmark().is_none() {
            self.status_message = String::from("No benchmark selected");
            return;
        }
        self.page = AppPage::BenchmarkDetail;
        self.selected_step_idx = 0;
        self.detail_focus = DetailFocus::Steps;
        self.diff_scroll = 0;
    }

    pub fn back_to_benchmark_list(&mut self) {
        self.page = AppPage::BenchmarkList;
    }

    pub fn clear_session(&mut self) {
        let Some(benchmark) = self.selected_benchmark().map(|b| b.name.clone()) else {
            self.status_message = String::from("No benchmark selected");
            return;
        };

        if self.sessions_by_benchmark.remove(&benchmark).is_some() {
            self.selected_step_idx = 0;
            self.diff_scroll = 0;
            self.status_message = String::from("Session cleared");
        } else {
            self.status_message = String::from("No session to clear");
        }
    }

    pub fn set_status_message(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
    }

    pub fn is_job_running(&self) -> bool {
        matches!(self.job_state, JobState::Running(_))
    }

    pub fn active_session_for_selected_benchmark(&self) -> Option<&RunSession> {
        let benchmark_name = self.selected_benchmark()?.name.as_str();
        self.sessions_by_benchmark.get(benchmark_name)
    }

    pub fn selected_step_index(&self) -> usize {
        let Some(session) = self.active_session_for_selected_benchmark() else {
            return 0;
        };
        if session.ir_diff_steps.is_empty() {
            return 0;
        }
        self.selected_step_idx
            .min(session.ir_diff_steps.len().saturating_sub(1))
    }

    pub fn selected_ir_diff_step(&self) -> Option<&IrDiffStep> {
        let session = self.active_session_for_selected_benchmark()?;
        if session.ir_diff_steps.is_empty() {
            return None;
        }
        session.ir_diff_steps.get(self.selected_step_index())
    }

    pub fn select_prev_step(&mut self) {
        let old = self.selected_step_idx;
        self.selected_step_idx = self.selected_step_idx.saturating_sub(1);
        if self.selected_step_idx != old {
            self.diff_scroll = 0;
        }
    }

    pub fn select_next_step(&mut self) {
        let Some(session) = self.active_session_for_selected_benchmark() else {
            return;
        };
        if session.ir_diff_steps.is_empty() {
            return;
        }
        let max_idx = session.ir_diff_steps.len() - 1;
        let old = self.selected_step_idx;
        self.selected_step_idx = (self.selected_step_idx + 1).min(max_idx);
        if self.selected_step_idx != old {
            self.diff_scroll = 0;
        }
    }

    pub fn focus_prev_tab(&mut self) {
        self.detail_focus = self.detail_focus.prev();
        self.status_message = format!("Focus: {}", self.detail_focus.label());
    }

    pub fn focus_next_tab(&mut self) {
        self.detail_focus = self.detail_focus.next();
        self.status_message = format!("Focus: {}", self.detail_focus.label());
    }

    pub fn detail_move_up(&mut self) {
        match self.detail_focus {
            DetailFocus::Steps => self.select_prev_step(),
            DetailFocus::IrDiff => self.scroll_diff_up(),
        }
    }

    pub fn detail_move_down(&mut self) {
        match self.detail_focus {
            DetailFocus::Steps => self.select_next_step(),
            DetailFocus::IrDiff => self.scroll_diff_down(),
        }
    }

    pub fn is_steps_focused(&self) -> bool {
        self.detail_focus == DetailFocus::Steps
    }

    pub fn is_ir_diff_focused(&self) -> bool {
        self.detail_focus == DetailFocus::IrDiff
    }

    fn scroll_diff_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(1);
    }

    fn scroll_diff_down(&mut self) {
        let max_scroll = self.max_diff_scroll();
        self.diff_scroll = self.diff_scroll.saturating_add(1).min(max_scroll);
    }

    fn max_diff_scroll(&self) -> u16 {
        let Some(step) = self.selected_ir_diff_step() else {
            return 0;
        };
        let max = step.diff_text.lines().count().saturating_sub(1);
        max.min(u16::MAX as usize) as u16
    }

    pub fn toggle_overlay(&mut self) {
        self.overlay_enabled = !self.overlay_enabled;
        self.status_message = if self.overlay_enabled {
            String::from("Overlay enabled")
        } else {
            String::from("Overlay disabled")
        };
    }

    pub fn begin_job(&mut self, kind: JobKind, benchmark: String, profile: CompileProfile) {
        self.job_state = JobState::Running(kind);
        self.running_benchmark = Some(benchmark.clone());
        self.sessions_by_benchmark.insert(
            benchmark.clone(),
            RunSession::new_running(profile, benchmark.clone()),
        );
        self.selected_step_idx = 0;
        self.diff_scroll = 0;
        self.status_message = format!("{kind} started for {benchmark} ({profile})");
    }

    pub fn handle_job_event(&mut self, event: JobEvent) {
        match event {
            JobEvent::Started {
                kind,
                benchmark,
                profile,
            } => {
                self.job_state = JobState::Running(kind);
                self.running_benchmark = Some(benchmark.clone());
                self.sessions_by_benchmark.insert(
                    benchmark.clone(),
                    RunSession::new_running(profile, benchmark.clone()),
                );
                self.status_message = format!("{kind} started for {benchmark} ({profile})");
            }
            JobEvent::LogLine(line) => {
                if let Some(benchmark) = self.running_benchmark.as_deref()
                    && let Some(session) = self.sessions_by_benchmark.get_mut(benchmark)
                {
                    session.logs.push(line);
                    const MAX_LOG_LINES: usize = 4000;
                    if session.logs.len() > MAX_LOG_LINES {
                        let overflow = session.logs.len() - MAX_LOG_LINES;
                        session.logs.drain(0..overflow);
                    }
                }
            }
            JobEvent::Finished(result) => {
                self.job_state = JobState::Idle;
                let running_benchmark = self.running_benchmark.take();
                match result {
                    Ok(outcome) => {
                        let mut session = self
                            .sessions_by_benchmark
                            .remove(&outcome.benchmark)
                            .unwrap_or_else(|| {
                                RunSession::new_running(outcome.profile, outcome.benchmark.clone())
                            });
                        session.profile = outcome.profile;
                        session.benchmark = outcome.benchmark.clone();
                        session.loop_results = outcome.loop_results;
                        session.remarks = outcome.remarks;
                        session.ir_diff_steps = outcome.ir_diff_steps;
                        session.optimization_steps = outcome.optimization_steps;
                        session.remarks_summary = outcome.remarks_summary;
                        session.status = SessionStatus::Succeeded;
                        self.sessions_by_benchmark
                            .insert(outcome.benchmark.clone(), session);
                        if self
                            .selected_benchmark()
                            .is_some_and(|b| b.name == outcome.benchmark)
                        {
                            self.selected_step_idx = 0;
                            self.diff_scroll = 0;
                        }
                        self.status_message =
                            format!("Completed: {} ({})", outcome.benchmark, outcome.profile);
                    }
                    Err(error) => {
                        if let Some(benchmark) = running_benchmark
                            && let Some(session) = self.sessions_by_benchmark.get_mut(&benchmark)
                        {
                            session.status = SessionStatus::Failed(error.clone());
                        }
                        self.status_message = format!("Job failed: {error}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RemarkKind;

    fn benchmark(name: &str) -> BenchmarkItem {
        BenchmarkItem {
            name: name.to_string(),
            category: String::from("Category"),
            data_type: String::from("dbl"),
            run_options: vec![String::from("100"), String::from("5")],
        }
    }

    fn step(pass: &str, idx: usize) -> OptimizationStep {
        OptimizationStep {
            pass: pass.to_string(),
            total: 1,
            passed: 1,
            missed: 0,
            analysis: 0,
            other: 0,
            remark_indices: vec![idx],
        }
    }

    fn outcome_for(benchmark: &str, profile: CompileProfile) -> JobOutcome {
        let remarks = vec![
            RemarkEntry {
                kind: RemarkKind::Passed,
                pass: String::from("licm"),
                name: String::from("Hoisted"),
                file: None,
                line: None,
                function: Some(String::from("main")),
                message: Some(String::from("ok")),
            },
            RemarkEntry {
                kind: RemarkKind::Missed,
                pass: String::from("loop-vectorize"),
                name: String::from("MissedDetails"),
                file: None,
                line: None,
                function: Some(String::from("main")),
                message: Some(String::from("no")),
            },
        ];
        JobOutcome {
            benchmark: benchmark.to_string(),
            profile,
            loop_results: vec![LoopResult {
                loop_id: String::from("S1"),
                time_sec: 1.0,
                checksum: String::from("123"),
            }],
            remarks_summary: RemarksSummary::from_entries(&remarks),
            ir_diff_steps: vec![IrDiffStep {
                index: 1,
                pass: String::from("LICMPass"),
                target: String::from("main"),
                changed_lines: 3,
                diff_text: String::from("@@ -1 +1 @@\n-old\n+new"),
                remark_indices: vec![0],
            }],
            optimization_steps: vec![step("licm", 0), step("loop-vectorize", 1)],
            remarks,
        }
    }

    #[test]
    fn page_navigation_roundtrip() {
        let mut app = AppState::new(vec![benchmark("A")]);
        assert_eq!(app.page, AppPage::BenchmarkList);
        app.open_selected_benchmark_page();
        assert_eq!(app.page, AppPage::BenchmarkDetail);
        app.back_to_benchmark_list();
        assert_eq!(app.page, AppPage::BenchmarkList);
    }

    #[test]
    fn optimization_step_selection_is_clamped() {
        let mut app = AppState::new(vec![benchmark("A")]);
        app.open_selected_benchmark_page();
        app.sessions_by_benchmark.insert(
            String::from("A"),
            RunSession {
                profile: CompileProfile::O3Remarks,
                benchmark: String::from("A"),
                loop_results: Vec::new(),
                remarks: Vec::new(),
                ir_diff_steps: vec![
                    IrDiffStep {
                        index: 1,
                        pass: String::from("LICMPass"),
                        target: String::from("main"),
                        changed_lines: 1,
                        diff_text: String::from("d1"),
                        remark_indices: Vec::new(),
                    },
                    IrDiffStep {
                        index: 2,
                        pass: String::from("LoopVectorizePass"),
                        target: String::from("main"),
                        changed_lines: 2,
                        diff_text: String::from("d2"),
                        remark_indices: Vec::new(),
                    },
                ],
                optimization_steps: vec![step("licm", 0), step("loop-vectorize", 1)],
                remarks_summary: RemarksSummary::default(),
                logs: Vec::new(),
                status: SessionStatus::Succeeded,
            },
        );

        app.select_next_step();
        app.select_next_step();
        assert_eq!(app.selected_step_index(), 1);

        app.select_prev_step();
        app.select_prev_step();
        assert_eq!(app.selected_step_index(), 0);
    }

    #[test]
    fn overlay_toggle_changes_state() {
        let mut app = AppState::new(vec![benchmark("A")]);
        assert!(!app.overlay_enabled);
        app.toggle_overlay();
        assert!(app.overlay_enabled);
        app.toggle_overlay();
        assert!(!app.overlay_enabled);
    }

    #[test]
    fn detail_focus_controls_up_down_behavior() {
        let mut app = AppState::new(vec![benchmark("A")]);
        app.open_selected_benchmark_page();
        app.sessions_by_benchmark.insert(
            String::from("A"),
            RunSession {
                profile: CompileProfile::O3Remarks,
                benchmark: String::from("A"),
                loop_results: Vec::new(),
                remarks: Vec::new(),
                ir_diff_steps: vec![
                    IrDiffStep {
                        index: 1,
                        pass: String::from("LICMPass"),
                        target: String::from("main"),
                        changed_lines: 1,
                        diff_text: String::from("line1\nline2\nline3"),
                        remark_indices: Vec::new(),
                    },
                    IrDiffStep {
                        index: 2,
                        pass: String::from("LoopVectorizePass"),
                        target: String::from("main"),
                        changed_lines: 1,
                        diff_text: String::from("line1\nline2\nline3\nline4"),
                        remark_indices: Vec::new(),
                    },
                ],
                optimization_steps: vec![step("licm", 0), step("loop-vectorize", 1)],
                remarks_summary: RemarksSummary::default(),
                logs: Vec::new(),
                status: SessionStatus::Succeeded,
            },
        );

        assert!(app.is_steps_focused());
        app.detail_move_down();
        assert_eq!(app.selected_step_index(), 1);
        assert_eq!(app.diff_scroll, 0);

        app.focus_next_tab();
        assert!(app.is_ir_diff_focused());
        app.detail_move_down();
        app.detail_move_down();
        assert_eq!(app.diff_scroll, 2);
        app.detail_move_up();
        assert_eq!(app.diff_scroll, 1);
    }

    #[test]
    fn finished_events_store_sessions_per_benchmark() {
        let mut app = AppState::new(vec![benchmark("A"), benchmark("B")]);

        app.handle_job_event(JobEvent::Started {
            kind: JobKind::BuildAndRun,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
        });
        app.handle_job_event(JobEvent::LogLine(String::from("log-a")));
        app.handle_job_event(JobEvent::Finished(Ok(outcome_for(
            "A",
            CompileProfile::O3Remarks,
        ))));

        app.select_next();
        app.handle_job_event(JobEvent::Started {
            kind: JobKind::Build,
            benchmark: String::from("B"),
            profile: CompileProfile::O3Default,
        });
        app.handle_job_event(JobEvent::Finished(Ok(outcome_for(
            "B",
            CompileProfile::O3Default,
        ))));

        assert_eq!(app.sessions_by_benchmark.len(), 2);
        assert_eq!(
            app.sessions_by_benchmark
                .get("A")
                .expect("session A should exist")
                .benchmark,
            "A"
        );
        assert_eq!(
            app.sessions_by_benchmark
                .get("B")
                .expect("session B should exist")
                .benchmark,
            "B"
        );
    }

    #[test]
    fn clear_session_only_affects_selected_benchmark() {
        let mut app = AppState::new(vec![benchmark("A"), benchmark("B")]);
        app.sessions_by_benchmark.insert(
            String::from("A"),
            RunSession::new_running(CompileProfile::O3Remarks, String::from("A")),
        );
        app.sessions_by_benchmark.insert(
            String::from("B"),
            RunSession::new_running(CompileProfile::O3Remarks, String::from("B")),
        );

        app.clear_session();
        assert!(!app.sessions_by_benchmark.contains_key("A"));
        assert!(app.sessions_by_benchmark.contains_key("B"));
    }
}
