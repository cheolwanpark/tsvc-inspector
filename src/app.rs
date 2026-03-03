use std::collections::HashMap;

use crate::model::{
    AppPage, BenchmarkFunction, BenchmarkItem, CompileProfile, FunctionRunMode, IrDiffStep,
    JobKind, LoopResult, OptimizationStep, RemarkEntry, RemarksSummary, RunSession, SessionStatus,
};

#[derive(Debug)]
pub enum JobEvent {
    Started {
        kind: JobKind,
        benchmark: String,
        profile: CompileProfile,
        selected_function: BenchmarkFunction,
        run_mode: FunctionRunMode,
    },
    LogLine(String),
    Finished(Result<JobOutcome, String>),
}

#[derive(Debug)]
pub struct JobOutcome {
    pub benchmark: String,
    pub profile: CompileProfile,
    pub selected_function: BenchmarkFunction,
    pub run_mode: FunctionRunMode,
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
pub enum ListFocus {
    Benchmarks,
    SourceCode,
}

impl ListFocus {
    fn next(self) -> Self {
        match self {
            Self::Benchmarks => Self::SourceCode,
            Self::SourceCode => Self::Benchmarks,
        }
    }

    fn prev(self) -> Self {
        self.next()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Benchmarks => "Benchmarks",
            Self::SourceCode => "Source Code",
        }
    }
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
    pub list_focus: ListFocus,
    pub list_source_scroll: u16,
    pub detail_focus: DetailFocus,
    pub diff_scroll: u16,
    pub function_modal_open: bool,
    pub function_modal_selected_idx: usize,
    pub function_run_mode: FunctionRunMode,
    selected_function_by_benchmark: HashMap<String, BenchmarkFunction>,
    sessions_by_key: HashMap<String, RunSession>,
    running_session_key: Option<String>,
}

impl AppState {
    pub fn new_with_run_mode(
        benchmarks: Vec<BenchmarkItem>,
        function_run_mode: FunctionRunMode,
    ) -> Self {
        Self {
            benchmarks,
            selected_idx: 0,
            active_profile: CompileProfile::O3Remarks,
            overlay_enabled: false,
            page: AppPage::BenchmarkList,
            selected_step_idx: 0,
            job_state: JobState::Idle,
            status_message: String::from("Ready"),
            list_focus: ListFocus::Benchmarks,
            list_source_scroll: 0,
            detail_focus: DetailFocus::Steps,
            diff_scroll: 0,
            function_modal_open: false,
            function_modal_selected_idx: 0,
            function_run_mode,
            selected_function_by_benchmark: HashMap::new(),
            sessions_by_key: HashMap::new(),
            running_session_key: None,
        }
    }

    pub fn selected_benchmark(&self) -> Option<&BenchmarkItem> {
        self.benchmarks.get(self.selected_idx)
    }

    pub fn selected_function_for_selected_benchmark(&self) -> Option<&BenchmarkFunction> {
        let benchmark_name = self.selected_benchmark()?.name.as_str();
        self.selected_function_by_benchmark.get(benchmark_name)
    }

    pub fn selected_function_loop_id(&self) -> Option<&str> {
        Some(
            self.selected_function_for_selected_benchmark()?
                .loop_id
                .as_str(),
        )
    }

    pub fn selected_function_symbol(&self) -> Option<&str> {
        Some(
            self.selected_function_for_selected_benchmark()?
                .symbol
                .as_str(),
        )
    }

    pub fn function_modal_items_for_selected_benchmark(&self) -> Option<&[BenchmarkFunction]> {
        let benchmark = self.selected_benchmark()?;
        Some(benchmark.available_functions.as_slice())
    }

    pub fn is_function_modal_open(&self) -> bool {
        self.function_modal_open
    }

    pub fn open_function_select_modal(&mut self) {
        let Some(benchmark) = self.selected_benchmark() else {
            self.status_message = String::from("No benchmark selected");
            return;
        };
        if benchmark.available_functions.is_empty() {
            self.status_message = String::from("No functions discovered for selected benchmark");
            return;
        }

        let selected_idx = self
            .selected_function_by_benchmark
            .get(&benchmark.name)
            .and_then(|selected| {
                benchmark
                    .available_functions
                    .iter()
                    .position(|entry| entry.symbol == selected.symbol)
            })
            .unwrap_or(0);

        self.function_modal_open = true;
        self.function_modal_selected_idx = selected_idx;
        self.status_message = String::from("Select function and press Enter");
    }

    pub fn close_function_select_modal(&mut self) {
        self.function_modal_open = false;
        self.status_message = String::from("Function selection canceled");
    }

    pub fn function_modal_move_up(&mut self) {
        self.function_modal_selected_idx = self.function_modal_selected_idx.saturating_sub(1);
    }

    pub fn function_modal_move_down(&mut self) {
        let Some(items) = self.function_modal_items_for_selected_benchmark() else {
            return;
        };
        if items.is_empty() {
            return;
        }
        let max_idx = items.len() - 1;
        self.function_modal_selected_idx = (self.function_modal_selected_idx + 1).min(max_idx);
    }

    pub fn confirm_function_selection(&mut self) {
        let Some(benchmark) = self.selected_benchmark().cloned() else {
            self.status_message = String::from("No benchmark selected");
            return;
        };
        if benchmark.available_functions.is_empty() {
            self.status_message = String::from("No functions discovered for selected benchmark");
            return;
        }

        let max_idx = benchmark.available_functions.len() - 1;
        let idx = self.function_modal_selected_idx.min(max_idx);
        let function = benchmark.available_functions[idx].clone();
        self.selected_function_by_benchmark
            .insert(benchmark.name.clone(), function.clone());

        self.function_modal_open = false;
        self.status_message = format!(
            "Selected function: {} ({})",
            function.loop_id, function.symbol
        );
        self.open_selected_benchmark_page();
    }

    pub fn select_prev(&mut self) {
        if self.benchmarks.is_empty() {
            return;
        }
        let old = self.selected_idx;
        self.selected_idx = self.selected_idx.saturating_sub(1);
        if self.selected_idx != old {
            self.list_source_scroll = 0;
        }
    }

    pub fn select_next(&mut self) {
        if self.benchmarks.is_empty() {
            return;
        }
        let max_idx = self.benchmarks.len() - 1;
        let old = self.selected_idx;
        self.selected_idx = (self.selected_idx + 1).min(max_idx);
        if self.selected_idx != old {
            self.list_source_scroll = 0;
        }
    }

    pub fn list_move_up(&mut self) {
        match self.list_focus {
            ListFocus::Benchmarks => self.select_prev(),
            ListFocus::SourceCode => self.scroll_source_up(),
        }
    }

    pub fn list_move_down(&mut self) {
        match self.list_focus {
            ListFocus::Benchmarks => self.select_next(),
            ListFocus::SourceCode => self.scroll_source_down(),
        }
    }

    pub fn focus_prev_list_pane(&mut self) {
        self.list_focus = self.list_focus.prev();
        self.status_message = format!("Focus: {}", self.list_focus.label());
    }

    pub fn focus_next_list_pane(&mut self) {
        self.list_focus = self.list_focus.next();
        self.status_message = format!("Focus: {}", self.list_focus.label());
    }

    pub fn is_benchmarks_focused(&self) -> bool {
        self.list_focus == ListFocus::Benchmarks
    }

    pub fn is_source_code_focused(&self) -> bool {
        self.list_focus == ListFocus::SourceCode
    }

    fn scroll_source_up(&mut self) {
        self.list_source_scroll = self.list_source_scroll.saturating_sub(1);
    }

    fn scroll_source_down(&mut self) {
        let max_scroll = self.max_source_scroll();
        self.list_source_scroll = self.list_source_scroll.saturating_add(1).min(max_scroll);
    }

    fn max_source_scroll(&self) -> u16 {
        let Some(benchmark) = self.selected_benchmark() else {
            return 0;
        };
        let max = benchmark.source_code.lines().count().saturating_sub(1);
        max.min(u16::MAX as usize) as u16
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
        if self.selected_function_for_selected_benchmark().is_none() {
            self.status_message = String::from("Select a function first");
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
        let Some(benchmark) = self.selected_benchmark() else {
            self.status_message = String::from("No benchmark selected");
            return;
        };
        let Some(function) = self.selected_function_for_selected_benchmark() else {
            self.status_message = String::from("Select a function first");
            return;
        };
        let key = session_key(&benchmark.name, &function.symbol);

        if self.sessions_by_key.remove(&key).is_some() {
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
        let benchmark = self.selected_benchmark()?;
        let function = self.selected_function_for_selected_benchmark()?;
        self.sessions_by_key
            .get(&session_key(&benchmark.name, &function.symbol))
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

    pub fn begin_job(
        &mut self,
        kind: JobKind,
        benchmark: String,
        profile: CompileProfile,
        selected_function: BenchmarkFunction,
        run_mode: FunctionRunMode,
    ) {
        self.job_state = JobState::Running(kind);
        self.selected_function_by_benchmark
            .insert(benchmark.clone(), selected_function.clone());

        let key = session_key(&benchmark, &selected_function.symbol);
        self.running_session_key = Some(key.clone());
        self.sessions_by_key.insert(
            key,
            RunSession::new_running(
                profile,
                benchmark.clone(),
                selected_function.loop_id.clone(),
                selected_function.symbol.clone(),
                run_mode,
            ),
        );

        self.selected_step_idx = 0;
        self.diff_scroll = 0;
        self.status_message = format!(
            "{kind} started for {benchmark} [{}] ({profile})",
            selected_function.loop_id
        );
    }

    pub fn handle_job_event(&mut self, event: JobEvent) {
        match event {
            JobEvent::Started {
                kind,
                benchmark,
                profile,
                selected_function,
                run_mode,
            } => {
                self.begin_job(kind, benchmark, profile, selected_function, run_mode);
            }
            JobEvent::LogLine(line) => {
                if let Some(session_key) = self.running_session_key.as_deref()
                    && let Some(session) = self.sessions_by_key.get_mut(session_key)
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
                let running_session_key = self.running_session_key.take();
                match result {
                    Ok(outcome) => {
                        let key =
                            session_key(&outcome.benchmark, &outcome.selected_function.symbol);
                        let mut session = self.sessions_by_key.remove(&key).unwrap_or_else(|| {
                            RunSession::new_running(
                                outcome.profile,
                                outcome.benchmark.clone(),
                                outcome.selected_function.loop_id.clone(),
                                outcome.selected_function.symbol.clone(),
                                outcome.run_mode,
                            )
                        });

                        session.profile = outcome.profile;
                        session.benchmark = outcome.benchmark.clone();
                        session.selected_function_loop_id =
                            outcome.selected_function.loop_id.clone();
                        session.selected_function_symbol = outcome.selected_function.symbol.clone();
                        session.run_mode = outcome.run_mode;
                        session.loop_results = outcome.loop_results;
                        session.remarks = outcome.remarks;
                        session.ir_diff_steps = outcome.ir_diff_steps;
                        session.optimization_steps = outcome.optimization_steps;
                        session.remarks_summary = outcome.remarks_summary;
                        session.status = SessionStatus::Succeeded;

                        self.selected_function_by_benchmark
                            .insert(outcome.benchmark.clone(), outcome.selected_function.clone());
                        self.sessions_by_key.insert(key, session);

                        if self
                            .selected_benchmark()
                            .is_some_and(|b| b.name == outcome.benchmark)
                            && self
                                .selected_function_for_selected_benchmark()
                                .is_some_and(|f| f.symbol == outcome.selected_function.symbol)
                        {
                            self.selected_step_idx = 0;
                            self.diff_scroll = 0;
                        }
                        self.status_message = format!(
                            "Completed: {} [{}] ({})",
                            outcome.benchmark, outcome.selected_function.loop_id, outcome.profile
                        );
                    }
                    Err(error) => {
                        if let Some(key) = running_session_key
                            && let Some(session) = self.sessions_by_key.get_mut(&key)
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

fn session_key(benchmark: &str, function_symbol: &str) -> String {
    format!("{benchmark}::{function_symbol}")
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
            available_functions: vec![
                BenchmarkFunction {
                    loop_id: String::from("S161"),
                    symbol: String::from("s161"),
                },
                BenchmarkFunction {
                    loop_id: String::from("S162"),
                    symbol: String::from("s162"),
                },
            ],
            source_code: String::from("line1\nline2\nline3\nline4"),
        }
    }

    fn outcome_for(
        benchmark: &str,
        profile: CompileProfile,
        function: BenchmarkFunction,
    ) -> JobOutcome {
        let remarks = vec![RemarkEntry {
            kind: RemarkKind::Passed,
            pass: String::from("licm"),
            name: String::from("Hoisted"),
            file: None,
            line: None,
            function: Some(function.symbol.clone()),
            message: Some(String::from("ok")),
        }];
        JobOutcome {
            benchmark: benchmark.to_string(),
            profile,
            selected_function: function,
            run_mode: FunctionRunMode::OutputFilter,
            loop_results: vec![LoopResult {
                loop_id: String::from("S161"),
                time_sec: 1.0,
                checksum: String::from("123"),
            }],
            remarks_summary: RemarksSummary::from_entries(&remarks),
            ir_diff_steps: vec![IrDiffStep {
                index: 1,
                pass: String::from("LICMPass"),
                target: String::from("s161"),
                changed_lines: 3,
                diff_text: String::from("@@ -1 +1 @@\n-old\n+new"),
                remark_indices: vec![0],
            }],
            optimization_steps: vec![OptimizationStep {
                pass: String::from("licm"),
                total: 1,
                passed: 1,
                missed: 0,
                analysis: 0,
                other: 0,
                remark_indices: vec![0],
            }],
            remarks,
        }
    }

    #[test]
    fn modal_selection_opens_detail_page() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_function_select_modal();
        assert!(app.is_function_modal_open());
        app.confirm_function_selection();
        assert_eq!(app.page, AppPage::BenchmarkDetail);
        assert_eq!(app.selected_function_loop_id(), Some("S161"));
    }

    #[test]
    fn detail_requires_function_selection() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_selected_benchmark_page();
        assert_eq!(app.page, AppPage::BenchmarkList);
        assert!(app.status_message.contains("Select a function first"));
    }

    #[test]
    fn sessions_are_scoped_per_function() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.selected_function_by_benchmark.insert(
            String::from("A"),
            BenchmarkFunction {
                loop_id: String::from("S161"),
                symbol: String::from("s161"),
            },
        );
        app.sessions_by_key.insert(
            String::from("A::s161"),
            RunSession::new_running(
                CompileProfile::O3Remarks,
                String::from("A"),
                String::from("S161"),
                String::from("s161"),
                FunctionRunMode::OutputFilter,
            ),
        );
        app.sessions_by_key.insert(
            String::from("A::s162"),
            RunSession::new_running(
                CompileProfile::O3Remarks,
                String::from("A"),
                String::from("S162"),
                String::from("s162"),
                FunctionRunMode::OutputFilter,
            ),
        );

        app.clear_session();
        assert!(!app.sessions_by_key.contains_key("A::s161"));
        assert!(app.sessions_by_key.contains_key("A::s162"));
    }

    #[test]
    fn finished_event_updates_selected_function_session() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        let selected_function = BenchmarkFunction {
            loop_id: String::from("S161"),
            symbol: String::from("s161"),
        };

        app.handle_job_event(JobEvent::Started {
            kind: JobKind::BuildAndRun,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(outcome_for(
            "A",
            CompileProfile::O3Remarks,
            selected_function,
        ))));

        let session = app
            .sessions_by_key
            .get("A::s161")
            .expect("session should exist");
        assert_eq!(session.selected_function_loop_id, "S161");
        assert!(matches!(session.status, SessionStatus::Succeeded));
    }
}
