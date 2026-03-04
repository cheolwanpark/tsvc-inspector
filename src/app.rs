use std::collections::HashMap;

use ratatui::style::Color;

use crate::model::{
    AnalysisStage, AnalysisState, AnalysisStep, AppPage, BenchmarkFunction, BenchmarkItem,
    CompileProfile, FunctionRunMode, JobKind, LoopResult, RemarkEntry, RemarksSummary, RunSession,
    SessionStatus,
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
    pub kind: JobKind,
    pub benchmark: String,
    pub profile: CompileProfile,
    pub selected_function: BenchmarkFunction,
    pub run_mode: FunctionRunMode,
    pub data: JobOutcomeData,
}

#[derive(Debug)]
pub enum JobOutcomeData {
    Runtime {
        loop_results: Vec<LoopResult>,
        remarks: Vec<RemarkEntry>,
        remarks_summary: RemarksSummary,
    },
    Analysis {
        analysis_steps: Vec<AnalysisStep>,
        remarks: Vec<RemarkEntry>,
        remarks_summary: RemarksSummary,
    },
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
    StageList,
    PassList,
    SourceView,
    IrView,
}

impl DetailFocus {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::StageList => "Stages",
            Self::PassList => "Passes",
            Self::SourceView => "C Source",
            Self::IrView => "IR View",
        }
    }

    /// Tab: cycle forward through all 4 panes, wrapping around.
    pub fn cycle_next(self) -> Self {
        match self {
            Self::StageList => Self::PassList,
            Self::PassList => Self::SourceView,
            Self::SourceView => Self::IrView,
            Self::IrView => Self::StageList,
        }
    }

    /// Shift-Tab: cycle backward through all 4 panes, wrapping around.
    pub fn cycle_prev(self) -> Self {
        match self {
            Self::StageList => Self::IrView,
            Self::PassList => Self::StageList,
            Self::SourceView => Self::PassList,
            Self::IrView => Self::SourceView,
        }
    }

}

pub struct AppState {
    pub benchmarks: Vec<BenchmarkItem>,
    pub selected_idx: usize,
    pub active_profile: CompileProfile,
    pub page: AppPage,
    pub selected_stage: AnalysisStage,
    pub selected_pass_by_stage: HashMap<AnalysisStage, usize>,
    pub job_state: JobState,
    pub status_message: String,
    pub list_focus: ListFocus,
    pub list_source_scroll: u16,
    pub detail_focus: DetailFocus,
    pub ir_scroll: u16,
    pub source_detail_scroll: u16,
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
            page: AppPage::BenchmarkList,
            selected_stage: AnalysisStage::Vectorize,
            selected_pass_by_stage: HashMap::new(),
            job_state: JobState::Idle,
            status_message: String::from("Ready"),
            list_focus: ListFocus::Benchmarks,
            list_source_scroll: 0,
            detail_focus: DetailFocus::StageList,
            ir_scroll: 0,
            source_detail_scroll: 0,
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

    #[allow(dead_code)]
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
        self.detail_focus = DetailFocus::StageList;
        self.ir_scroll = 0;
        self.source_detail_scroll = 0;
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
            self.ir_scroll = 0;
            self.source_detail_scroll = 0;
            self.selected_pass_by_stage.clear();
            self.detail_focus = DetailFocus::StageList;
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

    // --- Stage/Pass navigation ---

    /// Returns stages (sorted by pipeline_order) that have at least one pass, with their counts.
    pub fn ordered_stages_with_counts(session: &RunSession) -> Vec<(AnalysisStage, usize)> {
        let mut counts: HashMap<AnalysisStage, usize> = HashMap::new();
        for step in &session.analysis_steps {
            *counts.entry(step.stage).or_insert(0) += 1;
        }
        let mut result: Vec<(AnalysisStage, usize)> = counts.into_iter().collect();
        result.sort_by_key(|(stage, _)| stage.pipeline_order());
        result
    }

    /// Returns passes for the given stage in order.
    pub fn passes_for_stage(session: &RunSession, stage: AnalysisStage) -> Vec<&AnalysisStep> {
        session
            .analysis_steps
            .iter()
            .filter(|s| s.stage == stage)
            .collect()
    }

    /// Returns the selected pass index within the current stage, clamped to valid range.
    pub fn selected_pass_index_in_stage(&self, session: &RunSession) -> usize {
        let passes = Self::passes_for_stage(session, self.selected_stage);
        if passes.is_empty() {
            return 0;
        }
        let stored = self
            .selected_pass_by_stage
            .get(&self.selected_stage)
            .copied()
            .unwrap_or(0);
        stored.min(passes.len() - 1)
    }

    /// Returns a reference to the currently selected pass, or None if no passes exist.
    pub fn selected_step_in_stage<'a>(&self, session: &'a RunSession) -> Option<&'a AnalysisStep> {
        let passes = Self::passes_for_stage(session, self.selected_stage);
        if passes.is_empty() {
            return None;
        }
        let idx = self.selected_pass_index_in_stage(session);
        passes.into_iter().nth(idx)
    }

    pub fn is_stage_focused(&self) -> bool {
        self.detail_focus == DetailFocus::StageList
    }

    pub fn is_pass_focused(&self) -> bool {
        self.detail_focus == DetailFocus::PassList
    }

    pub fn is_source_view_focused(&self) -> bool {
        self.detail_focus == DetailFocus::SourceView
    }

    pub fn is_ir_view_focused(&self) -> bool {
        self.detail_focus == DetailFocus::IrView
    }

    /// Tab: cycle forward through all 4 panes.
    pub fn focus_cycle_next(&mut self) {
        self.detail_focus = self.detail_focus.cycle_next();
    }

    /// Shift-Tab: cycle backward through all 4 panes.
    pub fn focus_cycle_prev(&mut self) {
        self.detail_focus = self.detail_focus.cycle_prev();
    }

    pub fn select_prev_stage(&mut self) {
        let stages = self
            .active_session_for_selected_benchmark()
            .map(Self::ordered_stages_with_counts)
            .unwrap_or_default();
        if stages.is_empty() {
            return;
        }
        let current_pos = stages
            .iter()
            .position(|(s, _)| *s == self.selected_stage)
            .unwrap_or(0);
        if current_pos > 0 {
            self.selected_stage = stages[current_pos - 1].0;
        }
    }

    pub fn select_next_stage(&mut self) {
        let stages = self
            .active_session_for_selected_benchmark()
            .map(Self::ordered_stages_with_counts)
            .unwrap_or_default();
        if stages.is_empty() {
            return;
        }
        let current_pos = stages
            .iter()
            .position(|(s, _)| *s == self.selected_stage)
            .unwrap_or(0);
        if current_pos + 1 < stages.len() {
            self.selected_stage = stages[current_pos + 1].0;
        }
    }

    pub fn select_prev_pass(&mut self) {
        let count = self
            .active_session_for_selected_benchmark()
            .map(|s| Self::passes_for_stage(s, self.selected_stage).len())
            .unwrap_or(0);
        if count == 0 {
            return;
        }
        let idx = self
            .selected_pass_by_stage
            .get(&self.selected_stage)
            .copied()
            .unwrap_or(0);
        self.selected_pass_by_stage
            .insert(self.selected_stage, idx.saturating_sub(1));
        self.ir_scroll = 0;
    }

    pub fn select_next_pass(&mut self) {
        let count = self
            .active_session_for_selected_benchmark()
            .map(|s| Self::passes_for_stage(s, self.selected_stage).len())
            .unwrap_or(0);
        if count == 0 {
            return;
        }
        let idx = self
            .selected_pass_by_stage
            .get(&self.selected_stage)
            .copied()
            .unwrap_or(0);
        let new_idx = (idx + 1).min(count - 1);
        self.selected_pass_by_stage
            .insert(self.selected_stage, new_idx);
        self.ir_scroll = 0;
    }

    pub fn detail_move_up(&mut self) {
        match self.detail_focus {
            DetailFocus::StageList => self.select_prev_stage(),
            DetailFocus::PassList => self.select_prev_pass(),
            DetailFocus::SourceView => self.scroll_source_detail_up(),
            DetailFocus::IrView => self.scroll_ir_up(),
        }
    }

    pub fn detail_move_down(&mut self) {
        match self.detail_focus {
            DetailFocus::StageList => self.select_next_stage(),
            DetailFocus::PassList => self.select_next_pass(),
            DetailFocus::SourceView => self.scroll_source_detail_down(),
            DetailFocus::IrView => self.scroll_ir_down(),
        }
    }

    fn scroll_ir_up(&mut self) {
        self.ir_scroll = self.ir_scroll.saturating_sub(1);
    }

    fn scroll_ir_down(&mut self) {
        let max_scroll = self.max_ir_scroll();
        self.ir_scroll = self.ir_scroll.saturating_add(1).min(max_scroll);
    }

    fn max_ir_scroll(&self) -> u16 {
        let Some(session) = self.active_session_for_selected_benchmark() else {
            return 0;
        };
        let Some(step) = self.selected_step_in_stage(session) else {
            return 0;
        };
        let max = step.ir_lines.len().saturating_sub(1);
        max.min(u16::MAX as usize) as u16
    }

    fn scroll_source_detail_up(&mut self) {
        self.source_detail_scroll = self.source_detail_scroll.saturating_sub(1);
    }

    fn scroll_source_detail_down(&mut self) {
        let max_scroll = self.max_source_detail_scroll();
        self.source_detail_scroll = self.source_detail_scroll.saturating_add(1).min(max_scroll);
    }

    fn max_source_detail_scroll(&self) -> u16 {
        let Some(benchmark) = self.selected_benchmark() else {
            return 0;
        };
        let max = benchmark.source_code.lines().count().saturating_sub(1);
        max.min(u16::MAX as usize) as u16
    }

    /// When IR scroll changes, find source lines corresponding to visible IR region
    /// and auto-scroll C source to center those lines.
    /// Currently disabled: !dbg line numbers are absolute file positions but source_code
    /// is a kernel excerpt with different line numbering. Needs line offset tracking.
    #[allow(dead_code)]
    fn sync_source_to_ir(&mut self) {
        let Some(session) = self.active_session_for_selected_benchmark() else {
            return;
        };
        let Some(step) = self.selected_step_in_stage(session) else {
            return;
        };
        // Skip if no dbg metadata at all (entire map is None)
        if step.source_line_map.is_empty() || step.source_line_map.iter().all(|v| v.is_none()) {
            return;
        }

        let start = self.ir_scroll as usize;
        let end = (start + 20).min(step.source_line_map.len());
        let visible_source_lines: Vec<u32> = step.source_line_map[start..end]
            .iter()
            .filter_map(|opt| *opt)
            .collect();

        if visible_source_lines.is_empty() {
            return; // no matches in visible range — don't move source scroll
        }

        // Center on the median visible source line
        let mid = visible_source_lines[visible_source_lines.len() / 2];
        let max = self.max_source_detail_scroll();
        let target = mid.saturating_sub(5);
        self.source_detail_scroll = (target as u16).min(max);
    }

    /// After analysis completes, auto-navigate to Vectorize stage + PassList focus.
    /// Skipped if user has already navigated away from StageList.
    pub fn auto_navigate_to_vectorize(&mut self) {
        if self.detail_focus != DetailFocus::StageList {
            return;
        }
        let stages = self
            .active_session_for_selected_benchmark()
            .map(Self::ordered_stages_with_counts)
            .unwrap_or_default();
        if stages.is_empty() {
            return;
        }
        let target_stage = if stages.iter().any(|(s, _)| *s == AnalysisStage::Vectorize) {
            AnalysisStage::Vectorize
        } else {
            stages[0].0
        };
        self.selected_stage = target_stage;
        self.selected_pass_by_stage.insert(target_stage, 0);
        self.detail_focus = DetailFocus::PassList;
    }

    /// Returns a badge string and color for the benchmark's analysis state in the list page.
    pub fn verdict_badge_for_benchmark(&self, name: &str) -> Option<(String, Color)> {
        let function = self.selected_function_by_benchmark.get(name)?;
        let session = self
            .sessions_by_key
            .get(&session_key(name, &function.symbol))?;

        let badge = match session.analysis_state {
            AnalysisState::Running => ("⟳".to_string(), Color::Cyan),
            AnalysisState::Failed => ("!".to_string(), Color::Red),
            AnalysisState::Ready => {
                let summary = &session.remarks_summary;
                if summary.vectorized > 0 {
                    let vf = extract_vf_from_remarks(&session.remarks);
                    let text = match vf {
                        Some(n) => format!("✓ ×{n}"),
                        None => "✓".to_string(),
                    };
                    (text, Color::Green)
                } else if summary.missed_details > 0 {
                    ("✗".to_string(), Color::Red)
                } else if summary.not_beneficial > 0 {
                    ("○".to_string(), Color::Yellow)
                } else if has_vectorizer_ir_changes(session) {
                    ("~".to_string(), Color::Cyan)
                } else {
                    ("—".to_string(), Color::DarkGray)
                }
            }
            AnalysisState::None => return None,
        };
        Some(badge)
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
        let mut session = self.sessions_by_key.remove(&key).unwrap_or_else(|| {
            RunSession::new_running(
                profile,
                benchmark.clone(),
                selected_function.loop_id.clone(),
                selected_function.symbol.clone(),
                run_mode,
            )
        });
        session.profile = profile;
        session.benchmark = benchmark.clone();
        session.selected_function_loop_id = selected_function.loop_id.clone();
        session.selected_function_symbol = selected_function.symbol.clone();
        session.run_mode = run_mode;
        session.status = SessionStatus::Running;
        session.logs.clear();
        if matches!(kind, JobKind::AnalyzeFast) {
            session.remarks.clear();
            session.remarks_summary = RemarksSummary::default();
            session.analysis_steps.clear();
        }
        session.analysis_state = match kind {
            JobKind::AnalyzeFast => AnalysisState::Running,
            _ => session.analysis_state,
        };
        self.sessions_by_key.insert(key, session);

        self.ir_scroll = 0;
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
                let finished_kind = match self.job_state {
                    JobState::Running(kind) => Some(kind),
                    JobState::Idle => None,
                };
                self.job_state = JobState::Idle;
                let running_session_key = self.running_session_key.take();
                match result {
                    Ok(outcome) => {
                        let JobOutcome {
                            kind,
                            benchmark,
                            profile,
                            selected_function,
                            run_mode,
                            data,
                        } = outcome;
                        let key = session_key(&benchmark, &selected_function.symbol);
                        let mut session = self.sessions_by_key.remove(&key).unwrap_or_else(|| {
                            RunSession::new_running(
                                profile,
                                benchmark.clone(),
                                selected_function.loop_id.clone(),
                                selected_function.symbol.clone(),
                                run_mode,
                            )
                        });

                        session.profile = profile;
                        session.benchmark = benchmark.clone();
                        session.selected_function_loop_id = selected_function.loop_id.clone();
                        session.selected_function_symbol = selected_function.symbol.clone();
                        session.run_mode = run_mode;
                        match data {
                            JobOutcomeData::Runtime {
                                loop_results,
                                remarks,
                                remarks_summary,
                            } => {
                                session.loop_results = loop_results;
                                session.remarks = remarks;
                                session.remarks_summary = remarks_summary;
                                // After a runtime job, keep analysis state if steps exist
                                if !session.analysis_steps.is_empty() {
                                    session.analysis_state = AnalysisState::Ready;
                                } else {
                                    session.analysis_state = AnalysisState::None;
                                }
                            }
                            JobOutcomeData::Analysis {
                                analysis_steps,
                                remarks,
                                remarks_summary,
                            } => {
                                session.analysis_steps = analysis_steps;
                                session.remarks = remarks;
                                session.remarks_summary = remarks_summary;
                                session.analysis_state = AnalysisState::Ready;
                                self.status_message = format!(
                                    "Analysis ready: {} [{}]",
                                    benchmark, selected_function.loop_id
                                );
                            }
                        }
                        session.status = SessionStatus::Succeeded;

                        self.selected_function_by_benchmark
                            .insert(benchmark.clone(), selected_function.clone());
                        self.sessions_by_key.insert(key, session);

                        // Auto-navigate after analysis if this is the currently viewed benchmark
                        if matches!(kind, JobKind::AnalyzeFast)
                            && self
                                .selected_benchmark()
                                .is_some_and(|b| b.name == benchmark)
                            && self
                                .selected_function_for_selected_benchmark()
                                .is_some_and(|f| f.symbol == selected_function.symbol)
                        {
                            self.auto_navigate_to_vectorize();
                        }

                        if !matches!(kind, JobKind::AnalyzeFast) {
                            self.status_message = format!(
                                "Completed: {} [{}] ({})",
                                benchmark, selected_function.loop_id, profile
                            );
                        }
                    }
                    Err(error) => {
                        if let Some(key) = running_session_key
                            && let Some(session) = self.sessions_by_key.get_mut(&key)
                        {
                            session.status = SessionStatus::Failed(error.clone());
                            if matches!(finished_kind, Some(JobKind::AnalyzeFast)) {
                                session.analysis_state = AnalysisState::Failed;
                            }
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

/// Checks if vectorizer passes (loopvectorize/slpvectorizer) made IR changes.
pub fn has_vectorizer_ir_changes(session: &RunSession) -> bool {
    session.analysis_steps.iter().any(|step| {
        step.stage == AnalysisStage::Vectorize
            && step.changed_lines > 0
            && matches!(step.pass_key.as_str(), "loopvectorize" | "slpvectorizer")
    })
}

/// Extracts the vectorization factor from a session's remarks.
fn extract_vf_from_remarks(remarks: &[RemarkEntry]) -> Option<u32> {
    for r in remarks {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AnalysisSource, AnalysisStage, AnalysisStep, RemarkKind};

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

    fn make_step(stage: AnalysisStage, pass_key: &str, visible_index: usize) -> AnalysisStep {
        AnalysisStep {
            visible_index,
            raw_index: visible_index + 1,
            pass: pass_key.to_string(),
            pass_key: pass_key.to_string(),
            pass_occurrence: 1,
            stage,
            target_raw: String::from("s161"),
            target_function: Some(String::from("s161")),
            changed_lines: 3,
            diff_text: String::from("@@ -1 +1 @@\n-old\n+new"),
            ir_lines: vec![],
            source_line_map: vec![],
            remark_indices: vec![],
            source: AnalysisSource::TraceFast,
        }
    }

    fn outcome_for(
        benchmark_name: &str,
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
            kind: JobKind::AnalyzeFast,
            benchmark: benchmark_name.to_string(),
            profile,
            selected_function: function,
            run_mode: FunctionRunMode::OutputFilter,
            data: JobOutcomeData::Analysis {
                analysis_steps: vec![make_step(AnalysisStage::Loop, "licm", 0)],
                remarks_summary: RemarksSummary::from_entries(&remarks),
                remarks,
            },
        }
    }

    fn outcome_with_vectorize(
        benchmark_name: &str,
        profile: CompileProfile,
        function: BenchmarkFunction,
    ) -> JobOutcome {
        let remarks = vec![RemarkEntry {
            kind: RemarkKind::Passed,
            pass: String::from("loop-vectorize"),
            name: String::from("Vectorized"),
            file: None,
            line: None,
            function: Some(function.symbol.clone()),
            message: Some(String::from("vectorized loop (VF = 4)")),
        }];
        JobOutcome {
            kind: JobKind::AnalyzeFast,
            benchmark: benchmark_name.to_string(),
            profile,
            selected_function: function,
            run_mode: FunctionRunMode::OutputFilter,
            data: JobOutcomeData::Analysis {
                analysis_steps: vec![
                    make_step(AnalysisStage::Loop, "licm", 0),
                    make_step(AnalysisStage::Vectorize, "loop-vectorize", 1),
                ],
                remarks_summary: RemarksSummary::from_entries(&remarks),
                remarks,
            },
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
            kind: JobKind::AnalyzeFast,
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

    #[test]
    fn auto_navigates_to_vectorize_after_analysis() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        let selected_function = BenchmarkFunction {
            loop_id: String::from("S161"),
            symbol: String::from("s161"),
        };
        app.open_function_select_modal();
        app.confirm_function_selection();

        app.handle_job_event(JobEvent::Started {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(outcome_with_vectorize(
            "A",
            CompileProfile::O3Remarks,
            selected_function,
        ))));

        assert_eq!(app.selected_stage, AnalysisStage::Vectorize);
        assert_eq!(app.detail_focus, DetailFocus::PassList);
    }

    #[test]
    fn auto_navigate_skips_if_user_navigated() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        let selected_function = BenchmarkFunction {
            loop_id: String::from("S161"),
            symbol: String::from("s161"),
        };
        app.open_function_select_modal();
        app.confirm_function_selection();
        // User manually moved to PassList before analysis completes
        app.detail_focus = DetailFocus::PassList;

        app.handle_job_event(JobEvent::Started {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(outcome_with_vectorize(
            "A",
            CompileProfile::O3Remarks,
            selected_function,
        ))));

        // Should still be PassList, auto-navigate was skipped
        assert_eq!(app.detail_focus, DetailFocus::PassList);
        // But selected_stage was NOT changed by auto_navigate since it was skipped
        // (remains as default Vectorize which happens to match, but the key thing is no navigation happened)
    }

    #[test]
    fn stage_navigation_wraps_correctly() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        let selected_function = BenchmarkFunction {
            loop_id: String::from("S161"),
            symbol: String::from("s161"),
        };
        app.open_function_select_modal();
        app.confirm_function_selection();

        app.handle_job_event(JobEvent::Started {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.detail_focus = DetailFocus::StageList;
        app.handle_job_event(JobEvent::Finished(Ok(outcome_with_vectorize(
            "A",
            CompileProfile::O3Remarks,
            selected_function,
        ))));

        // Reset to start of stages for this test
        app.detail_focus = DetailFocus::StageList;
        let session = app.active_session_for_selected_benchmark().unwrap();
        let stages = AppState::ordered_stages_with_counts(session);
        app.selected_stage = stages[0].0;

        // Try to go before first stage - should clamp
        app.select_prev_stage();
        let session = app.active_session_for_selected_benchmark().unwrap();
        let stages = AppState::ordered_stages_with_counts(session);
        assert_eq!(app.selected_stage, stages[0].0, "should clamp at first stage");

        // Navigate to last stage and try to go beyond
        app.selected_stage = stages[stages.len() - 1].0;
        app.select_next_stage();
        assert_eq!(
            app.selected_stage,
            stages[stages.len() - 1].0,
            "should clamp at last stage"
        );
    }

    #[test]
    fn pass_index_clamps_after_reanalysis() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        let selected_function = BenchmarkFunction {
            loop_id: String::from("S161"),
            symbol: String::from("s161"),
        };
        app.open_function_select_modal();
        app.confirm_function_selection();

        // First analysis with 2 vectorize passes
        let remarks: Vec<RemarkEntry> = vec![];
        app.handle_job_event(JobEvent::Started {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(JobOutcome {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
            data: JobOutcomeData::Analysis {
                analysis_steps: vec![
                    make_step(AnalysisStage::Vectorize, "loop-vectorize", 0),
                    make_step(AnalysisStage::Vectorize, "slp-vectorize", 1),
                ],
                remarks: remarks.clone(),
                remarks_summary: RemarksSummary::from_entries(&remarks),
            },
        })));

        // Select second pass (index 1)
        app.selected_stage = AnalysisStage::Vectorize;
        app.selected_pass_by_stage.insert(AnalysisStage::Vectorize, 1);

        // Second analysis with only 1 vectorize pass
        app.detail_focus = DetailFocus::PassList; // user already navigated
        app.handle_job_event(JobEvent::Started {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(JobOutcome {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
            data: JobOutcomeData::Analysis {
                analysis_steps: vec![make_step(AnalysisStage::Vectorize, "loop-vectorize", 0)],
                remarks: remarks.clone(),
                remarks_summary: RemarksSummary::from_entries(&remarks),
            },
        })));

        // Index should clamp: stored=1, len=1, so min(1, 0) = 0
        let session = app.active_session_for_selected_benchmark().unwrap();
        let clamped = app.selected_pass_index_in_stage(session);
        assert_eq!(clamped, 0, "index should clamp to 0 after reanalysis reduced pass count");
    }

    #[test]
    fn empty_stage_not_in_ordered_list() {
        let session = RunSession::new_running(
            CompileProfile::O3Remarks,
            String::from("A"),
            String::from("S161"),
            String::from("s161"),
            FunctionRunMode::OutputFilter,
        );
        // No steps → ordered_stages_with_counts returns empty
        let stages = AppState::ordered_stages_with_counts(&session);
        assert!(stages.is_empty());
    }

    #[test]
    fn focus_tab_cycles_through_all_4_panes() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        assert_eq!(app.detail_focus, DetailFocus::StageList);

        app.focus_cycle_next();
        assert_eq!(app.detail_focus, DetailFocus::PassList);

        app.focus_cycle_next();
        assert_eq!(app.detail_focus, DetailFocus::SourceView);

        app.focus_cycle_next();
        assert_eq!(app.detail_focus, DetailFocus::IrView);

        // Wraps around
        app.focus_cycle_next();
        assert_eq!(app.detail_focus, DetailFocus::StageList);

        // Reverse cycle
        app.focus_cycle_prev();
        assert_eq!(app.detail_focus, DetailFocus::IrView);

        app.focus_cycle_prev();
        assert_eq!(app.detail_focus, DetailFocus::SourceView);

        app.focus_cycle_prev();
        assert_eq!(app.detail_focus, DetailFocus::PassList);

        app.focus_cycle_prev();
        assert_eq!(app.detail_focus, DetailFocus::StageList);
    }

    #[test]
    fn verdict_fallback_likely_vectorized() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        let selected_function = BenchmarkFunction {
            loop_id: String::from("S161"),
            symbol: String::from("s161"),
        };
        app.open_function_select_modal();
        app.confirm_function_selection();

        // Create outcome with vectorizer IR changes but no vectorize remarks
        let remarks: Vec<RemarkEntry> = vec![];
        let mut step = make_step(AnalysisStage::Vectorize, "loopvectorize", 0);
        step.changed_lines = 10;

        app.handle_job_event(JobEvent::Started {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(JobOutcome {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            profile: CompileProfile::O3Remarks,
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
            data: JobOutcomeData::Analysis {
                analysis_steps: vec![step],
                remarks: remarks.clone(),
                remarks_summary: RemarksSummary::from_entries(&remarks),
            },
        })));

        let badge = app.verdict_badge_for_benchmark("A");
        assert!(badge.is_some());
        let (text, color) = badge.unwrap();
        assert_eq!(text, "~");
        assert_eq!(color, Color::Cyan);
    }
}
