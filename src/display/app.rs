use std::collections::HashMap;

use ratatui::style::Color;

use crate::core::model::{
    AnalysisStage, AnalysisState, AnalysisStep, AppPage, BenchmarkFunction, BenchmarkItem,
    CompilerConfig, FunctionRunMode, IrLine, JobKind, RemarkEntry, RemarksSummary, RunSession,
    SessionStatus,
};
use crate::transform::session::{
    DetailSnapshotInput, build_detail_snapshot, extract_vf_from_remarks, has_vectorizer_ir_changes,
};
use crate::transform::source::extract_c_function_source;

#[derive(Debug)]
pub enum JobEvent {
    Started {
        kind: JobKind,
        benchmark: String,
        compiler_config: CompilerConfig,
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
    pub compiler_config: CompilerConfig,
    pub selected_function: BenchmarkFunction,
    pub run_mode: FunctionRunMode,
    pub data: JobOutcomeData,
}

#[derive(Debug)]
pub enum JobOutcomeData {
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
    Selector,
    CodeView,
}

impl DetailFocus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Selector => "Pass Selector",
            Self::CodeView => "Code View",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeViewMode {
    IrDiff,
    IrPostPass,
    CSource,
}

impl CodeViewMode {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::IrDiff => Self::IrPostPass,
            Self::IrPostPass => Self::IrDiff,
            Self::CSource => Self::IrPostPass,
        }
    }

    pub fn cycle_prev(self) -> Self {
        match self {
            Self::IrDiff => Self::IrPostPass,
            Self::IrPostPass => Self::IrDiff,
            Self::CSource => Self::IrDiff,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::IrDiff => "IR Diff",
            Self::IrPostPass => "IR",
            Self::CSource => "C",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigRow {
    OptLevel,
    FastMath,
    NoInlining,
    LoopVectorize,
    SlpVectorize,
    ForceVecWidth,
    ForceInterleave,
    UnrollLoops,
    LoopInterchange,
    LoopDistribute,
    MarchNative,
    ExtraCFlags,
    ExtraLlvmFlags,
}

impl ConfigRow {
    pub const ALL: [Self; 13] = [
        Self::OptLevel,
        Self::FastMath,
        Self::NoInlining,
        Self::LoopVectorize,
        Self::SlpVectorize,
        Self::ForceVecWidth,
        Self::ForceInterleave,
        Self::UnrollLoops,
        Self::LoopInterchange,
        Self::LoopDistribute,
        Self::MarchNative,
        Self::ExtraCFlags,
        Self::ExtraLlvmFlags,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::OptLevel => "Optimization",
            Self::FastMath => "Fast Math",
            Self::NoInlining => "No Inlining",
            Self::LoopVectorize => "Loop Vectorize",
            Self::SlpVectorize => "SLP Vectorize",
            Self::ForceVecWidth => "Force Vec Width",
            Self::ForceInterleave => "Force Interleave",
            Self::UnrollLoops => "Unroll Loops",
            Self::LoopInterchange => "Loop Interchange",
            Self::LoopDistribute => "Loop Distribute",
            Self::MarchNative => "March Native",
            Self::ExtraCFlags => "Extra C Flags",
            Self::ExtraLlvmFlags => "Extra LLVM Flags",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            Self::OptLevel | Self::FastMath | Self::NoInlining => "Optimization",
            Self::LoopVectorize
            | Self::SlpVectorize
            | Self::ForceVecWidth
            | Self::ForceInterleave => "Vectorization",
            Self::UnrollLoops | Self::LoopInterchange | Self::LoopDistribute => "Loop Transforms",
            Self::MarchNative => "Target",
            Self::ExtraCFlags | Self::ExtraLlvmFlags => "Advanced",
        }
    }

    pub fn selectable_count() -> usize {
        Self::ALL.len()
    }

    pub fn from_index(idx: usize) -> Self {
        Self::ALL[idx.min(Self::ALL.len() - 1)]
    }
}

pub struct AppState {
    pub benchmarks: Vec<BenchmarkItem>,
    pub selected_idx: usize,
    pub config_draft: CompilerConfig,
    pub config_modal_open: bool,
    pub config_selected_row: usize,
    pub config_editing_text: bool,
    pub page: AppPage,
    pub selected_stage: AnalysisStage,
    pub selected_pass_by_stage: HashMap<AnalysisStage, usize>,
    pub job_state: JobState,
    pub status_message: String,
    pub list_focus: ListFocus,
    pub list_source_scroll: u16,
    pub detail_focus: DetailFocus,
    pub code_view_mode: CodeViewMode,
    last_ir_code_view_mode: CodeViewMode,
    pub ir_scroll: u16,
    pub ir_diff_selected_line: usize,
    pub ir_post_selected_line: usize,
    pub source_detail_scroll: u16,
    pub detail_code_viewport_lines: u16,
    pub side_by_side_diff_open: bool,
    pub side_by_side_diff_scroll: u16,
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
            config_draft: CompilerConfig::default(),
            config_modal_open: false,
            config_selected_row: 0,
            config_editing_text: false,
            page: AppPage::BenchmarkList,
            selected_stage: AnalysisStage::Initial,
            selected_pass_by_stage: HashMap::new(),
            job_state: JobState::Idle,
            status_message: String::from("Ready"),
            list_focus: ListFocus::Benchmarks,
            list_source_scroll: 0,
            detail_focus: DetailFocus::Selector,
            code_view_mode: CodeViewMode::IrDiff,
            last_ir_code_view_mode: CodeViewMode::IrDiff,
            ir_scroll: 0,
            ir_diff_selected_line: 0,
            ir_post_selected_line: 0,
            source_detail_scroll: 0,
            detail_code_viewport_lines: 1,
            side_by_side_diff_open: false,
            side_by_side_diff_scroll: 0,
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

    pub fn current_compiler_config(&self) -> CompilerConfig {
        self.config_draft.clone()
    }

    pub fn detail_source_text_for_selected_benchmark(&self) -> Option<String> {
        let benchmark = self.selected_benchmark()?;
        let function = self.selected_function_for_selected_benchmark()?;
        Some(
            extract_c_function_source(&benchmark.source_code, &function.symbol).unwrap_or_else(
                || {
                    format!(
                        "(source unavailable: could not locate function '{}' in kernel-focused source)",
                        function.symbol
                    )
                },
            ),
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

    pub fn confirm_function_selection(&mut self) -> bool {
        let Some(benchmark) = self.selected_benchmark().cloned() else {
            self.status_message = String::from("No benchmark selected");
            return false;
        };
        if benchmark.available_functions.is_empty() {
            self.status_message = String::from("No functions discovered for selected benchmark");
            return false;
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
        self.page == AppPage::BenchmarkDetail
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

    pub fn list_move_left(&mut self) {
        self.list_focus = self.list_focus.prev();
        self.status_message = format!("Focus: {}", self.list_focus.label());
    }

    pub fn list_move_right(&mut self) {
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

    pub fn open_config_modal(&mut self) {
        if self.page != AppPage::BenchmarkList {
            return;
        }
        self.config_modal_open = true;
        self.config_editing_text = false;
        self.status_message = String::from("Configuration modal opened");
    }

    pub fn close_config_modal(&mut self) {
        if !self.config_modal_open {
            return;
        }
        self.config_modal_open = false;
        self.config_editing_text = false;
        self.status_message = String::from("Configuration modal closed");
    }

    pub fn is_config_modal_open(&self) -> bool {
        self.config_modal_open
    }

    pub fn config_rows(&self) -> &'static [ConfigRow] {
        &ConfigRow::ALL
    }

    pub fn is_config_text_editing(&self) -> bool {
        self.config_editing_text
    }

    pub fn config_selected_row_kind(&self) -> ConfigRow {
        ConfigRow::from_index(self.config_selected_row)
    }

    pub fn config_row_value_text(&self, row: ConfigRow) -> String {
        match row {
            ConfigRow::OptLevel => self.config_draft.opt_level.to_string(),
            ConfigRow::FastMath => bool_text(self.config_draft.fast_math),
            ConfigRow::NoInlining => bool_text(self.config_draft.no_inlining),
            ConfigRow::LoopVectorize => bool_text(self.config_draft.enable_loop_vectorize),
            ConfigRow::SlpVectorize => bool_text(self.config_draft.enable_slp_vectorize),
            ConfigRow::ForceVecWidth => self.config_draft.force_vector_width.to_string(),
            ConfigRow::ForceInterleave => self.config_draft.force_vector_interleave.to_string(),
            ConfigRow::UnrollLoops => bool_text(self.config_draft.unroll_loops),
            ConfigRow::LoopInterchange => bool_text(self.config_draft.loop_interchange),
            ConfigRow::LoopDistribute => bool_text(self.config_draft.loop_distribute),
            ConfigRow::MarchNative => bool_text(self.config_draft.march_native),
            ConfigRow::ExtraCFlags => {
                if self.config_draft.extra_c_flags.trim().is_empty() {
                    String::from("(empty)")
                } else {
                    self.config_draft.extra_c_flags.clone()
                }
            }
            ConfigRow::ExtraLlvmFlags => {
                if self.config_draft.extra_llvm_flags.trim().is_empty() {
                    String::from("(empty)")
                } else {
                    self.config_draft.extra_llvm_flags.clone()
                }
            }
        }
    }

    pub fn config_analysis_flags_preview(&self) -> String {
        let flags = self.config_draft.analysis_c_flags();
        if flags.is_empty() {
            String::from("(none)")
        } else {
            flags.join(" ")
        }
    }

    pub fn config_move_up(&mut self) {
        if self.config_editing_text {
            return;
        }
        self.config_selected_row = self.config_selected_row.saturating_sub(1);
    }

    pub fn config_move_down(&mut self) {
        if self.config_editing_text {
            return;
        }
        let max_idx = ConfigRow::selectable_count() - 1;
        self.config_selected_row = (self.config_selected_row + 1).min(max_idx);
    }

    pub fn config_confirm(&mut self) {
        let row = self.config_selected_row_kind();
        if self.config_editing_text {
            self.config_editing_text = false;
            self.status_message = String::from("Config text updated");
            return;
        }

        match row {
            ConfigRow::ExtraCFlags | ConfigRow::ExtraLlvmFlags => {
                self.config_editing_text = true;
                self.status_message = String::from("Editing text field (Enter to finish)");
            }
            _ => {
                self.adjust_config_row(row, true);
                self.status_message = format!("Config: {}", self.config_draft.label());
            }
        }
    }

    pub fn cancel_config_text_edit(&mut self) {
        if !self.config_editing_text {
            return;
        }
        self.config_editing_text = false;
        self.status_message = String::from("Canceled text editing");
    }

    pub fn config_push_char(&mut self, ch: char) {
        if !self.config_editing_text {
            return;
        }
        match self.config_selected_row_kind() {
            ConfigRow::ExtraCFlags => self.config_draft.extra_c_flags.push(ch),
            ConfigRow::ExtraLlvmFlags => self.config_draft.extra_llvm_flags.push(ch),
            _ => {}
        }
    }

    pub fn config_backspace(&mut self) {
        if !self.config_editing_text {
            return;
        }
        match self.config_selected_row_kind() {
            ConfigRow::ExtraCFlags => {
                self.config_draft.extra_c_flags.pop();
            }
            ConfigRow::ExtraLlvmFlags => {
                self.config_draft.extra_llvm_flags.pop();
            }
            _ => {}
        }
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
        self.detail_focus = DetailFocus::Selector;
        self.code_view_mode = CodeViewMode::IrDiff;
        self.last_ir_code_view_mode = CodeViewMode::IrDiff;
        self.side_by_side_diff_open = false;
        self.side_by_side_diff_scroll = 0;
        self.ensure_valid_pass_selection_for_active_session();
        self.reset_ir_navigation();
    }

    pub fn back_to_benchmark_list(&mut self) {
        self.side_by_side_diff_open = false;
        self.side_by_side_diff_scroll = 0;
        self.page = AppPage::BenchmarkList;
    }

    fn adjust_config_row(&mut self, row: ConfigRow, forward: bool) {
        match row {
            ConfigRow::OptLevel => {
                if forward {
                    self.config_draft.opt_level = self.config_draft.opt_level.next();
                } else {
                    // Cycle backwards by moving forward five times (6-item enum).
                    for _ in 0..5 {
                        self.config_draft.opt_level = self.config_draft.opt_level.next();
                    }
                }
            }
            ConfigRow::FastMath => {
                self.config_draft.fast_math = !self.config_draft.fast_math;
            }
            ConfigRow::NoInlining => {
                self.config_draft.no_inlining = !self.config_draft.no_inlining;
            }
            ConfigRow::LoopVectorize => {
                self.config_draft.enable_loop_vectorize = !self.config_draft.enable_loop_vectorize;
            }
            ConfigRow::SlpVectorize => {
                self.config_draft.enable_slp_vectorize = !self.config_draft.enable_slp_vectorize;
            }
            ConfigRow::ForceVecWidth => {
                if forward {
                    self.config_draft.force_vector_width =
                        self.config_draft.force_vector_width.next();
                } else {
                    // Cycle backwards: 4 forward steps in a 5-item cycle.
                    for _ in 0..4 {
                        self.config_draft.force_vector_width =
                            self.config_draft.force_vector_width.next();
                    }
                }
            }
            ConfigRow::ForceInterleave => {
                if forward {
                    self.config_draft.force_vector_interleave =
                        self.config_draft.force_vector_interleave.next();
                } else {
                    // Cycle backwards: 3 forward steps in a 4-item cycle.
                    for _ in 0..3 {
                        self.config_draft.force_vector_interleave =
                            self.config_draft.force_vector_interleave.next();
                    }
                }
            }
            ConfigRow::UnrollLoops => {
                self.config_draft.unroll_loops = !self.config_draft.unroll_loops;
            }
            ConfigRow::LoopInterchange => {
                self.config_draft.loop_interchange = !self.config_draft.loop_interchange;
            }
            ConfigRow::LoopDistribute => {
                self.config_draft.loop_distribute = !self.config_draft.loop_distribute;
            }
            ConfigRow::MarchNative => {
                self.config_draft.march_native = !self.config_draft.march_native;
            }
            ConfigRow::ExtraCFlags | ConfigRow::ExtraLlvmFlags => {}
        }
        self.status_message = format!("Config: {}", self.config_draft.label());
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
        let config = self.current_compiler_config();
        self.sessions_by_key.get(&session_key(
            &benchmark.name,
            &function.symbol,
            &config.config_id(),
        ))
    }

    pub fn set_detail_code_viewport_lines(&mut self, lines: u16) {
        self.detail_code_viewport_lines = lines.max(1);
        self.normalize_ir_cursor();
    }

    pub fn selected_ir_visible_index(&self) -> usize {
        match self.code_view_mode {
            CodeViewMode::IrDiff => self.ir_diff_selected_line,
            CodeViewMode::IrPostPass => self.ir_post_selected_line,
            CodeViewMode::CSource => 0,
        }
    }

    pub fn visible_ir_line_count(&self) -> usize {
        let Some(session) = self.active_session_for_selected_benchmark() else {
            return 0;
        };
        let Some(step) = self.selected_step_in_stage(session) else {
            return 0;
        };
        Self::visible_ir_line_count_for_mode(step, self.code_view_mode)
    }

    pub fn visible_ir_line_for_selected_step(&self, idx: usize) -> Option<&IrLine> {
        let session = self.active_session_for_selected_benchmark()?;
        let step = self.selected_step_in_stage(session)?;
        Self::visible_ir_line_for_mode(step, self.code_view_mode, idx)
    }

    pub fn selected_ir_line_for_selected_step(&self) -> Option<&IrLine> {
        self.visible_ir_line_for_selected_step(self.selected_ir_visible_index())
    }

    fn visible_ir_line_count_for_mode(step: &AnalysisStep, mode: CodeViewMode) -> usize {
        match mode {
            CodeViewMode::IrDiff => step.ir_lines.len(),
            CodeViewMode::IrPostPass => step
                .ir_lines
                .iter()
                .filter(|line| !matches!(line.tag, similar::ChangeTag::Delete))
                .count(),
            CodeViewMode::CSource => 0,
        }
    }

    fn visible_ir_line_for_mode(
        step: &AnalysisStep,
        mode: CodeViewMode,
        idx: usize,
    ) -> Option<&IrLine> {
        match mode {
            CodeViewMode::IrDiff => step.ir_lines.get(idx),
            CodeViewMode::IrPostPass => step
                .ir_lines
                .iter()
                .filter(|line| !matches!(line.tag, similar::ChangeTag::Delete))
                .nth(idx),
            CodeViewMode::CSource => None,
        }
    }

    fn first_selectable_ir_index_for_mode(&self, mode: CodeViewMode) -> usize {
        let Some(session) = self.active_session_for_selected_benchmark() else {
            return 0;
        };
        let Some(step) = self.selected_step_in_stage(session) else {
            return 0;
        };
        match mode {
            CodeViewMode::IrDiff => step
                .ir_lines
                .iter()
                .position(|line| !line.is_source_annotation)
                .unwrap_or(0),
            CodeViewMode::IrPostPass => step
                .ir_lines
                .iter()
                .filter(|line| !matches!(line.tag, similar::ChangeTag::Delete))
                .position(|line| !line.is_source_annotation)
                .unwrap_or(0),
            CodeViewMode::CSource => 0,
        }
    }

    fn reset_ir_navigation(&mut self) {
        self.ir_scroll = 0;
        self.source_detail_scroll = 0;
        self.ir_diff_selected_line = self.first_selectable_ir_index_for_mode(CodeViewMode::IrDiff);
        self.ir_post_selected_line =
            self.first_selectable_ir_index_for_mode(CodeViewMode::IrPostPass);
        self.normalize_ir_cursor();
    }

    fn normalize_ir_cursor(&mut self) {
        if self.code_view_mode == CodeViewMode::CSource {
            return;
        }

        let line_count = self.visible_ir_line_count();
        if line_count == 0 {
            self.ir_scroll = 0;
            match self.code_view_mode {
                CodeViewMode::IrDiff => self.ir_diff_selected_line = 0,
                CodeViewMode::IrPostPass => self.ir_post_selected_line = 0,
                CodeViewMode::CSource => {}
            }
            return;
        }

        let max_idx = line_count - 1;
        match self.code_view_mode {
            CodeViewMode::IrDiff => {
                self.ir_diff_selected_line = self.ir_diff_selected_line.min(max_idx)
            }
            CodeViewMode::IrPostPass => {
                self.ir_post_selected_line = self.ir_post_selected_line.min(max_idx)
            }
            CodeViewMode::CSource => {}
        }
        self.ir_scroll = self.ir_scroll.min(max_idx.min(u16::MAX as usize) as u16);
        self.ensure_selected_ir_line_visible();
    }

    fn ensure_selected_ir_line_visible(&mut self) {
        if self.code_view_mode == CodeViewMode::CSource {
            return;
        }

        let selected = self.selected_ir_visible_index();
        let viewport = self.detail_code_viewport_lines.max(1) as usize;
        let scroll = self.ir_scroll as usize;
        if selected < scroll {
            self.ir_scroll = selected.min(u16::MAX as usize) as u16;
            return;
        }
        if selected >= scroll.saturating_add(viewport) {
            let target = selected + 1 - viewport;
            self.ir_scroll = target.min(u16::MAX as usize) as u16;
        }
    }

    fn move_ir_cursor_up(&mut self) {
        match self.code_view_mode {
            CodeViewMode::IrDiff => {
                self.ir_diff_selected_line = self.ir_diff_selected_line.saturating_sub(1)
            }
            CodeViewMode::IrPostPass => {
                self.ir_post_selected_line = self.ir_post_selected_line.saturating_sub(1)
            }
            CodeViewMode::CSource => return,
        }
        self.ensure_selected_ir_line_visible();
    }

    fn move_ir_cursor_down(&mut self) {
        let max_idx = self.visible_ir_line_count().saturating_sub(1);
        match self.code_view_mode {
            CodeViewMode::IrDiff => {
                self.ir_diff_selected_line = (self.ir_diff_selected_line + 1).min(max_idx)
            }
            CodeViewMode::IrPostPass => {
                self.ir_post_selected_line = (self.ir_post_selected_line + 1).min(max_idx)
            }
            CodeViewMode::CSource => return,
        }
        self.ensure_selected_ir_line_visible();
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

    pub fn ordered_pass_positions(session: &RunSession) -> Vec<(AnalysisStage, usize)> {
        let mut positions = Vec::new();
        for (stage, _) in Self::ordered_stages_with_counts(session) {
            let count = Self::passes_for_stage(session, stage).len();
            for pass_idx in 0..count {
                positions.push((stage, pass_idx));
            }
        }
        positions
    }

    fn ensure_valid_pass_selection_for_active_session(&mut self) {
        let Some(positions) = self
            .active_session_for_selected_benchmark()
            .map(Self::ordered_pass_positions)
        else {
            return;
        };
        let Some((first_stage, first_pass_idx)) = positions.first().copied() else {
            return;
        };

        let current = (
            self.selected_stage,
            self.selected_pass_by_stage
                .get(&self.selected_stage)
                .copied()
                .unwrap_or(0),
        );
        if positions.contains(&current) {
            return;
        }

        self.selected_stage = first_stage;
        self.selected_pass_by_stage
            .insert(first_stage, first_pass_idx);
    }

    fn selected_global_pass_position(&self, session: &RunSession) -> Option<usize> {
        let positions = Self::ordered_pass_positions(session);
        if positions.is_empty() {
            return None;
        }
        let current = (
            self.selected_stage,
            self.selected_pass_index_in_stage(session),
        );
        positions
            .iter()
            .position(|entry| *entry == current)
            .or(Some(0))
    }

    pub fn is_selector_focused(&self) -> bool {
        self.detail_focus == DetailFocus::Selector
    }

    pub fn is_code_view_focused(&self) -> bool {
        self.detail_focus == DetailFocus::CodeView
    }

    pub fn is_side_by_side_diff_open(&self) -> bool {
        self.side_by_side_diff_open
    }

    pub fn toggle_side_by_side_diff(&mut self) {
        if self.side_by_side_diff_open {
            self.close_side_by_side_diff();
            self.status_message = String::from("Side-by-side diff closed");
            return;
        }

        let Some(session) = self.active_session_for_selected_benchmark() else {
            self.status_message =
                String::from("Analysis results are required for side-by-side diff");
            return;
        };
        let Some(step) = self.selected_step_in_stage(session) else {
            self.status_message = String::from("Select a pass to open side-by-side diff");
            return;
        };
        if step.ir_lines.is_empty() {
            self.status_message = String::from("No IR diff available for side-by-side view");
            return;
        }

        self.side_by_side_diff_open = true;
        self.side_by_side_diff_scroll = 0;
        self.status_message = String::from("Side-by-side diff opened");
    }

    pub fn close_side_by_side_diff(&mut self) {
        self.side_by_side_diff_open = false;
        self.side_by_side_diff_scroll = 0;
    }

    pub fn side_by_side_diff_scroll_up(&mut self) {
        self.side_by_side_diff_scroll = self.side_by_side_diff_scroll.saturating_sub(1);
    }

    pub fn side_by_side_diff_scroll_down(&mut self) {
        let max_scroll = self.max_side_by_side_diff_scroll();
        self.side_by_side_diff_scroll = self
            .side_by_side_diff_scroll
            .saturating_add(1)
            .min(max_scroll);
    }

    pub fn detail_move_left(&mut self) {
        self.detail_focus = DetailFocus::Selector;
    }

    pub fn detail_move_right(&mut self) {
        self.detail_focus = DetailFocus::CodeView;
    }

    pub fn rotate_code_view_mode_next(&mut self) {
        self.code_view_mode = if self.code_view_mode == CodeViewMode::CSource {
            self.last_ir_code_view_mode
        } else {
            self.code_view_mode.cycle_next()
        };
        self.last_ir_code_view_mode = self.code_view_mode;
        self.reset_ir_navigation();
    }

    pub fn rotate_code_view_mode_prev(&mut self) {
        self.code_view_mode = if self.code_view_mode == CodeViewMode::CSource {
            self.last_ir_code_view_mode
        } else {
            self.code_view_mode.cycle_prev()
        };
        self.last_ir_code_view_mode = self.code_view_mode;
        self.reset_ir_navigation();
    }

    pub fn show_c_source_mode(&mut self) {
        if self.code_view_mode != CodeViewMode::CSource {
            self.last_ir_code_view_mode = self.code_view_mode;
        }
        self.code_view_mode = CodeViewMode::CSource;
        self.reset_ir_navigation();
    }

    pub fn select_prev_pass(&mut self) {
        let Some(session) = self.active_session_for_selected_benchmark() else {
            return;
        };
        let ordered = Self::ordered_pass_positions(session);
        if ordered.is_empty() {
            return;
        }
        let current = self.selected_global_pass_position(session).unwrap_or(0);
        let target = current.saturating_sub(1);
        let (stage, pass_idx) = ordered[target];
        self.selected_stage = stage;
        self.selected_pass_by_stage.insert(stage, pass_idx);
        self.reset_ir_navigation();
    }

    pub fn select_next_pass(&mut self) {
        let Some(session) = self.active_session_for_selected_benchmark() else {
            return;
        };
        let ordered = Self::ordered_pass_positions(session);
        if ordered.is_empty() {
            return;
        }
        let current = self.selected_global_pass_position(session).unwrap_or(0);
        let target = (current + 1).min(ordered.len() - 1);
        let (stage, pass_idx) = ordered[target];
        self.selected_stage = stage;
        self.selected_pass_by_stage.insert(stage, pass_idx);
        self.reset_ir_navigation();
    }

    pub fn detail_move_up(&mut self) {
        if self.side_by_side_diff_open {
            self.side_by_side_diff_scroll_up();
            return;
        }
        match self.detail_focus {
            DetailFocus::Selector => self.select_prev_pass(),
            DetailFocus::CodeView => {
                if self.code_view_mode == CodeViewMode::CSource {
                    self.scroll_source_detail_up();
                } else {
                    self.move_ir_cursor_up();
                }
            }
        }
    }

    pub fn detail_move_down(&mut self) {
        if self.side_by_side_diff_open {
            self.side_by_side_diff_scroll_down();
            return;
        }
        match self.detail_focus {
            DetailFocus::Selector => self.select_next_pass(),
            DetailFocus::CodeView => {
                if self.code_view_mode == CodeViewMode::CSource {
                    self.scroll_source_detail_down();
                } else {
                    self.move_ir_cursor_down();
                }
            }
        }
    }

    pub fn build_detail_copy_payload(&self) -> Result<String, String> {
        let benchmark = self
            .selected_benchmark()
            .ok_or_else(|| String::from("no benchmark selected"))?;
        let selected_function = self
            .selected_function_for_selected_benchmark()
            .ok_or_else(|| String::from("no function selected"))?;
        let session = self
            .active_session_for_selected_benchmark()
            .ok_or_else(|| String::from("no active session for selected function"))?;
        let step = self
            .selected_step_in_stage(session)
            .ok_or_else(|| String::from("no selected analysis pass"))?;
        let passes = Self::passes_for_stage(session, self.selected_stage);
        let selected_pass_index = self.selected_pass_index_in_stage(session) + 1;
        let source_text = self
            .detail_source_text_for_selected_benchmark()
            .unwrap_or_else(|| String::from("(source not available)"));
        Ok(build_detail_snapshot(DetailSnapshotInput {
            benchmark,
            selected_function,
            session,
            selected_stage: self.selected_stage,
            detail_focus_label: self.detail_focus.label(),
            step,
            selected_pass_index,
            passes_len: passes.len(),
            source_text: &source_text,
        }))
    }

    fn scroll_source_detail_up(&mut self) {
        self.source_detail_scroll = self.source_detail_scroll.saturating_sub(1);
    }

    fn scroll_source_detail_down(&mut self) {
        let max_scroll = self.max_source_detail_scroll();
        self.source_detail_scroll = self.source_detail_scroll.saturating_add(1).min(max_scroll);
    }

    fn max_source_detail_scroll(&self) -> u16 {
        let Some(source) = self.detail_source_text_for_selected_benchmark() else {
            return 0;
        };
        let max = source.lines().count().saturating_sub(1);
        max.min(u16::MAX as usize) as u16
    }

    fn max_side_by_side_diff_scroll(&self) -> u16 {
        let Some(session) = self.active_session_for_selected_benchmark() else {
            return 0;
        };
        let Some(step) = self.selected_step_in_stage(session) else {
            return 0;
        };
        let max = step.ir_lines.len().saturating_sub(1);
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

    /// Returns a badge string and color for the benchmark's analysis state in the list page.
    pub fn verdict_badge_for_benchmark(&self, name: &str) -> Option<(String, Color)> {
        let function = self.selected_function_by_benchmark.get(name)?;
        let config = self.current_compiler_config();
        let session =
            self.sessions_by_key
                .get(&session_key(name, &function.symbol, &config.config_id()))?;

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
        compiler_config: CompilerConfig,
        selected_function: BenchmarkFunction,
        run_mode: FunctionRunMode,
    ) {
        self.job_state = JobState::Running(kind);
        self.selected_function_by_benchmark
            .insert(benchmark.clone(), selected_function.clone());

        let config_id = compiler_config.config_id();
        let key = session_key(&benchmark, &selected_function.symbol, &config_id);
        self.running_session_key = Some(key.clone());
        let mut session = self.sessions_by_key.remove(&key).unwrap_or_else(|| {
            RunSession::new_running(
                compiler_config.clone(),
                benchmark.clone(),
                selected_function.loop_id.clone(),
                selected_function.symbol.clone(),
                run_mode,
            )
        });
        session.compiler_config = compiler_config.clone();
        session.config_id = config_id;
        session.benchmark = benchmark.clone();
        session.selected_function_loop_id = selected_function.loop_id.clone();
        session.selected_function_symbol = selected_function.symbol.clone();
        session.run_mode = run_mode;
        session.status = SessionStatus::Running;
        session.logs.clear();
        session.remarks.clear();
        session.remarks_summary = RemarksSummary::default();
        session.analysis_steps.clear();
        session.analysis_state = AnalysisState::Running;
        self.sessions_by_key.insert(key, session);

        self.reset_ir_navigation();
        self.status_message = format!(
            "{kind} started for {benchmark} [{}] ({})",
            selected_function.loop_id, compiler_config
        );
    }

    pub fn handle_job_event(&mut self, event: JobEvent) {
        match event {
            JobEvent::Started {
                kind,
                benchmark,
                compiler_config,
                selected_function,
                run_mode,
            } => {
                self.begin_job(
                    kind,
                    benchmark,
                    compiler_config,
                    selected_function,
                    run_mode,
                );
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
                            kind: _,
                            benchmark,
                            compiler_config,
                            selected_function,
                            run_mode,
                            data,
                        } = outcome;
                        let config_id = compiler_config.config_id();
                        let key = session_key(&benchmark, &selected_function.symbol, &config_id);
                        let mut session = self.sessions_by_key.remove(&key).unwrap_or_else(|| {
                            RunSession::new_running(
                                compiler_config.clone(),
                                benchmark.clone(),
                                selected_function.loop_id.clone(),
                                selected_function.symbol.clone(),
                                run_mode,
                            )
                        });

                        session.compiler_config = compiler_config.clone();
                        session.config_id = config_id;
                        session.benchmark = benchmark.clone();
                        session.selected_function_loop_id = selected_function.loop_id.clone();
                        session.selected_function_symbol = selected_function.symbol.clone();
                        session.run_mode = run_mode;
                        match data {
                            JobOutcomeData::Analysis {
                                analysis_steps,
                                remarks,
                                remarks_summary,
                            } => {
                                session.analysis_steps = analysis_steps;
                                session.remarks = remarks;
                                session.remarks_summary = remarks_summary;
                                session.analysis_state = AnalysisState::Ready;
                                self.reset_ir_navigation();
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

                        // Keep the detail selector on a valid pass for the currently viewed session.
                        if self
                            .selected_benchmark()
                            .is_some_and(|b| b.name == benchmark)
                            && self
                                .selected_function_for_selected_benchmark()
                                .is_some_and(|f| f.symbol == selected_function.symbol)
                        {
                            self.ensure_valid_pass_selection_for_active_session();
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

fn session_key(benchmark: &str, function_symbol: &str, config_id: &str) -> String {
    format!("{benchmark}::{function_symbol}::{config_id}")
}

fn bool_text(v: bool) -> String {
    if v {
        String::from("on")
    } else {
        String::from("off")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{
        AnalysisSource, AnalysisStage, AnalysisState, AnalysisStep, IrLine, RemarkKind,
    };
    use similar::ChangeTag;

    fn benchmark(name: &str) -> BenchmarkItem {
        BenchmarkItem {
            name: name.to_string(),
            category: String::from("Category"),
            data_type: String::from("dbl"),
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

    fn benchmark_with_source(name: &str, source_code: &str) -> BenchmarkItem {
        let mut item = benchmark(name);
        item.source_code = source_code.to_string();
        item
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

    fn attach_ready_session(app: &mut AppState, step: AnalysisStep, remarks: Vec<RemarkEntry>) {
        let benchmark = app
            .selected_benchmark()
            .expect("benchmark should exist")
            .name
            .clone();
        let selected_function = app
            .selected_function_for_selected_benchmark()
            .expect("selected function should exist")
            .clone();

        let config = app.current_compiler_config();
        let mut session = RunSession::new_running(
            config.clone(),
            benchmark.clone(),
            selected_function.loop_id.clone(),
            selected_function.symbol.clone(),
            FunctionRunMode::OutputFilter,
        );
        session.analysis_state = AnalysisState::Ready;
        session.analysis_steps = vec![step];
        session.remarks = remarks.clone();
        session.remarks_summary = RemarksSummary::from_entries(&remarks);
        session.status = SessionStatus::Succeeded;
        session.config_id = config.config_id();

        let key = session_key(&benchmark, &selected_function.symbol, &config.config_id());
        app.sessions_by_key.insert(key, session);
        app.ensure_valid_pass_selection_for_active_session();
    }

    #[test]
    fn extract_c_function_source_returns_only_target_function() {
        let source = r#"
int s160() {
    return 0;
}

int s161() {
    int acc = 0;
    acc += 1;
    return acc;
}

int s162() {
    return 2;
}
"#;

        let extracted =
            extract_c_function_source(source, "s161").expect("target function should be found");
        assert!(extracted.contains("int s161()"));
        assert!(extracted.contains("return acc;"));
        assert!(!extracted.contains("int s160()"));
        assert!(!extracted.contains("int s162()"));
    }

    #[test]
    fn extract_c_function_source_ignores_callsite_and_prototype() {
        let source = r#"
int main(void) {
    s161(7);
    return 0;
}

int s161(int n);

int s161(int n) {
    return n + 1;
}
"#;

        let extracted =
            extract_c_function_source(source, "s161").expect("definition should be found");
        assert!(extracted.contains("int s161(int n) {"));
        assert!(extracted.contains("return n + 1;"));
        assert!(!extracted.contains("int main(void)"));
    }

    #[test]
    fn extract_c_function_source_handles_multiline_signature() {
        let source = r#"
int
s161(
    int n,
    int m
)
{
    return n + m;
}
"#;

        let extracted =
            extract_c_function_source(source, "s161").expect("multiline signature should parse");
        assert!(extracted.contains("s161("));
        assert!(extracted.contains("return n + m;"));
    }

    #[test]
    fn detail_source_accessor_returns_unavailable_message_when_missing() {
        let mut app = AppState::new_with_run_mode(
            vec![benchmark_with_source(
                "A",
                "int other(void) { return 0; }\n",
            )],
            FunctionRunMode::OutputFilter,
        );
        app.selected_function_by_benchmark.insert(
            String::from("A"),
            BenchmarkFunction {
                loop_id: String::from("S161"),
                symbol: String::from("s161"),
            },
        );

        let detail_source = app
            .detail_source_text_for_selected_benchmark()
            .expect("detail source should resolve");
        assert_eq!(
            detail_source,
            "(source unavailable: could not locate function 's161' in kernel-focused source)"
        );
    }

    #[test]
    fn max_source_detail_scroll_uses_function_only_line_count() {
        let source = r#"
int helper(void) {
    return 0;
}

int s161() {
    int x = 0;
    x += 1;
    return x;
}

int tail(void) {
    return 1;
}
"#;
        let mut app = AppState::new_with_run_mode(
            vec![benchmark_with_source("A", source)],
            FunctionRunMode::OutputFilter,
        );
        app.selected_function_by_benchmark.insert(
            String::from("A"),
            BenchmarkFunction {
                loop_id: String::from("S161"),
                symbol: String::from("s161"),
            },
        );

        assert_eq!(app.max_source_detail_scroll(), 4);
    }

    #[test]
    fn build_detail_copy_payload_requires_active_session() {
        let source = r#"
int s161(void) {
    return 1;
}
"#;
        let mut app = AppState::new_with_run_mode(
            vec![benchmark_with_source("A", source)],
            FunctionRunMode::OutputFilter,
        );
        app.open_function_select_modal();
        app.confirm_function_selection();

        let err = app
            .build_detail_copy_payload()
            .expect_err("missing session should fail");
        assert_eq!(err, "no active session for selected function");
    }

    #[test]
    fn build_detail_copy_payload_contains_context_remarks_and_source() {
        let source = r#"
int s161(void) {
    return 1;
}
"#;
        let mut app = AppState::new_with_run_mode(
            vec![benchmark_with_source("A", source)],
            FunctionRunMode::OutputFilter,
        );
        app.open_function_select_modal();
        app.confirm_function_selection();

        let mut step = make_step(AnalysisStage::Vectorize, "loopvectorize", 0);
        step.pass = String::from("LoopVectorizePass");
        step.diff_text = String::from("@@ -1,3 +1,3 @@\n x\n-old\n+new\n y");
        step.ir_lines = vec![
            IrLine {
                tag: ChangeTag::Equal,
                text: String::from("x"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Delete,
                text: String::from("old"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Insert,
                text: String::from("new"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Equal,
                text: String::from("y"),
                is_source_annotation: false,
                details: Default::default(),
            },
        ];
        step.source_line_map = vec![None; step.ir_lines.len()];
        step.remark_indices = vec![0];

        let remarks = vec![RemarkEntry {
            kind: RemarkKind::Passed,
            pass: String::from("loop-vectorize"),
            name: String::from("Vectorized"),
            file: None,
            line: None,
            function: Some(String::from("s161")),
            message: Some(String::from("vectorized loop (VF = 4)")),
        }];
        attach_ready_session(&mut app, step, remarks);

        let payload = app
            .build_detail_copy_payload()
            .expect("payload should be generated");
        assert!(payload.contains("benchmark: A"));
        assert!(payload.contains("function: S161 (s161)"));
        assert!(payload.contains("stage: Vectorize"));
        assert!(payload.contains("pass_key: loopvectorize"));
        assert!(payload.contains("[Passed] loop-vectorize::Vectorized vectorized loop (VF = 4)"));
        assert!(payload.contains("int s161(void) {"));
        assert!(payload.contains("IR Diff (LoopVectorizePass)"));
        assert!(payload.contains("  x"));
        assert!(payload.contains("- old"));
        assert!(payload.contains("+ new"));
    }

    #[test]
    fn build_detail_copy_payload_includes_all_ir_lines_for_selected_pass() {
        let source = r#"
int s161(void) {
    return 1;
}
"#;
        let mut app = AppState::new_with_run_mode(
            vec![benchmark_with_source("A", source)],
            FunctionRunMode::OutputFilter,
        );
        app.open_function_select_modal();
        app.confirm_function_selection();
        let mut step = make_step(AnalysisStage::Vectorize, "loopvectorize", 0);
        step.ir_lines = vec![
            IrLine {
                tag: ChangeTag::Equal,
                text: String::from("l1"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Delete,
                text: String::from("old1"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Insert,
                text: String::from("new1"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Equal,
                text: String::from("l2"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Delete,
                text: String::from("old2"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Insert,
                text: String::from("new2"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Equal,
                text: String::from("l3"),
                is_source_annotation: false,
                details: Default::default(),
            },
        ];
        step.source_line_map = vec![None; step.ir_lines.len()];
        attach_ready_session(&mut app, step, vec![]);

        let payload = app
            .build_detail_copy_payload()
            .expect("payload should be generated");

        assert!(payload.contains("  l1"));
        assert!(payload.contains("- old1"));
        assert!(payload.contains("+ new1"));
        assert!(payload.contains("  l2"));
        assert!(payload.contains("- old2"));
        assert!(payload.contains("+ new2"));
        assert!(payload.contains("  l3"));
    }

    fn outcome_for(
        benchmark_name: &str,
        config: CompilerConfig,
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
            compiler_config: config,
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
        config: CompilerConfig,
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
            compiler_config: config,
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
        assert!(app.confirm_function_selection());
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
            compiler_config: CompilerConfig::default(),
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(outcome_for(
            "A",
            CompilerConfig::default(),
            selected_function,
        ))));

        let cfg = app.current_compiler_config();
        let session = app
            .sessions_by_key
            .get(&session_key("A", "s161", &cfg.config_id()))
            .expect("session should exist");
        assert_eq!(session.selected_function_loop_id, "S161");
        assert!(matches!(session.status, SessionStatus::Succeeded));
    }

    #[test]
    fn analysis_selects_first_available_stage_without_changing_focus() {
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
            compiler_config: CompilerConfig::default(),
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(outcome_with_vectorize(
            "A",
            CompilerConfig::default(),
            selected_function,
        ))));

        assert_eq!(app.selected_stage, AnalysisStage::Loop);
        assert_eq!(
            app.selected_pass_index_in_stage(
                app.active_session_for_selected_benchmark()
                    .expect("session should exist")
            ),
            0
        );
        assert_eq!(app.detail_focus, DetailFocus::Selector);
    }

    #[test]
    fn analysis_preserves_code_view_focus_while_normalizing_selection() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        let selected_function = BenchmarkFunction {
            loop_id: String::from("S161"),
            symbol: String::from("s161"),
        };
        app.open_function_select_modal();
        app.confirm_function_selection();
        // User manually moved focus to code view before analysis completes
        app.detail_focus = DetailFocus::CodeView;

        app.handle_job_event(JobEvent::Started {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            compiler_config: CompilerConfig::default(),
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(outcome_with_vectorize(
            "A",
            CompilerConfig::default(),
            selected_function,
        ))));

        assert_eq!(app.detail_focus, DetailFocus::CodeView);
        assert_eq!(app.selected_stage, AnalysisStage::Loop);
    }

    #[test]
    fn analysis_without_vectorize_stage_still_selects_available_pass() {
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
            compiler_config: CompilerConfig::default(),
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(outcome_for(
            "A",
            CompilerConfig::default(),
            selected_function,
        ))));

        let session = app
            .active_session_for_selected_benchmark()
            .expect("session should exist");
        assert_eq!(app.selected_stage, AnalysisStage::Loop);
        assert!(app.selected_step_in_stage(session).is_some());
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
            compiler_config: CompilerConfig::default(),
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(JobOutcome {
            kind: JobKind::AnalyzeFast,
            benchmark: String::from("A"),
            compiler_config: CompilerConfig::default(),
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

    #[test]
    fn config_modal_text_editing_flow() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_config_modal();
        assert!(app.is_config_modal_open());

        app.config_selected_row = ConfigRow::ALL
            .iter()
            .position(|row| *row == ConfigRow::ExtraCFlags)
            .unwrap();
        app.config_confirm();
        assert!(app.is_config_text_editing());
        app.config_push_char('-');
        app.config_push_char('X');
        app.config_backspace();
        app.config_push_char('g');
        app.config_confirm();

        assert_eq!(app.config_draft.extra_c_flags, "-g");
        assert!(!app.is_config_text_editing());
    }

    #[test]
    fn config_modal_toggles_no_inlining() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_config_modal();
        app.config_selected_row = ConfigRow::ALL
            .iter()
            .position(|row| *row == ConfigRow::NoInlining)
            .unwrap();

        app.config_confirm();

        assert!(app.config_draft.no_inlining);
    }

    #[test]
    fn config_modal_opens_without_function_selection() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        assert!(app.selected_function_for_selected_benchmark().is_none());
        app.open_config_modal();
        assert!(app.is_config_modal_open());
    }

    #[test]
    fn pass_navigation_moves_across_stage_boundaries() {
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
            compiler_config: CompilerConfig::default(),
            selected_function: selected_function.clone(),
            run_mode: FunctionRunMode::OutputFilter,
        });
        app.handle_job_event(JobEvent::Finished(Ok(outcome_with_vectorize(
            "A",
            CompilerConfig::default(),
            selected_function,
        ))));

        app.select_next_pass();
        assert_eq!(app.selected_stage, AnalysisStage::Vectorize);
        app.select_prev_pass();
        assert_eq!(app.selected_stage, AnalysisStage::Loop);
    }

    #[test]
    fn opening_detail_with_existing_session_selects_first_available_pass() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.selected_function_by_benchmark.insert(
            String::from("A"),
            BenchmarkFunction {
                loop_id: String::from("S161"),
                symbol: String::from("s161"),
            },
        );
        let step = make_step(AnalysisStage::Cleanup, "instcombine", 0);
        attach_ready_session(&mut app, step, vec![]);

        app.open_selected_benchmark_page();

        let session = app
            .active_session_for_selected_benchmark()
            .expect("session should exist");
        assert_eq!(app.selected_stage, AnalysisStage::Cleanup);
        assert!(app.selected_step_in_stage(session).is_some());
    }

    #[test]
    fn code_view_moves_ir_cursor_and_scrolls_with_viewport() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_function_select_modal();
        app.confirm_function_selection();

        let mut step = make_step(AnalysisStage::Vectorize, "loopvectorize", 0);
        step.ir_lines = (0..5)
            .map(|idx| IrLine {
                tag: ChangeTag::Equal,
                text: format!("line {idx}"),
                is_source_annotation: false,
                details: Default::default(),
            })
            .collect();
        step.source_line_map = vec![None; step.ir_lines.len()];
        attach_ready_session(&mut app, step, vec![]);

        app.detail_focus = DetailFocus::CodeView;
        app.code_view_mode = CodeViewMode::IrDiff;
        app.set_detail_code_viewport_lines(2);

        app.detail_move_down();
        assert_eq!(app.selected_ir_visible_index(), 1);
        assert_eq!(app.ir_scroll, 0);

        app.detail_move_down();
        assert_eq!(app.selected_ir_visible_index(), 2);
        assert_eq!(app.ir_scroll, 1);

        app.detail_move_down();
        assert_eq!(app.selected_ir_visible_index(), 3);
        assert_eq!(app.ir_scroll, 2);
    }

    #[test]
    fn c_source_mode_keeps_existing_scroll_behavior() {
        let source = r#"
int s161(void) {
    int a = 0;
    a += 1;
    a += 2;
    return a;
}
"#;
        let mut app = AppState::new_with_run_mode(
            vec![benchmark_with_source("A", source)],
            FunctionRunMode::OutputFilter,
        );
        app.open_function_select_modal();
        app.confirm_function_selection();
        app.detail_focus = DetailFocus::CodeView;
        app.code_view_mode = CodeViewMode::CSource;

        app.detail_move_down();
        app.detail_move_down();

        assert_eq!(app.source_detail_scroll, 2);
        assert_eq!(app.ir_diff_selected_line, 0);
        assert_eq!(app.ir_post_selected_line, 0);
    }

    #[test]
    fn tab_only_toggles_between_ir_diff_and_ir_modes() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_function_select_modal();
        app.confirm_function_selection();

        assert_eq!(app.code_view_mode, CodeViewMode::IrDiff);

        app.rotate_code_view_mode_next();
        assert_eq!(app.code_view_mode, CodeViewMode::IrPostPass);

        app.rotate_code_view_mode_next();
        assert_eq!(app.code_view_mode, CodeViewMode::IrDiff);

        app.rotate_code_view_mode_prev();
        assert_eq!(app.code_view_mode, CodeViewMode::IrPostPass);
    }

    #[test]
    fn c_source_mode_returns_to_last_ir_mode_on_tab() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_function_select_modal();
        app.confirm_function_selection();

        app.rotate_code_view_mode_next();
        assert_eq!(app.code_view_mode, CodeViewMode::IrPostPass);

        app.show_c_source_mode();
        assert_eq!(app.code_view_mode, CodeViewMode::CSource);

        app.rotate_code_view_mode_next();
        assert_eq!(app.code_view_mode, CodeViewMode::IrPostPass);
    }

    #[test]
    fn c_source_mode_returns_to_ir_diff_on_tab_when_it_was_last_seen() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_function_select_modal();
        app.confirm_function_selection();

        assert_eq!(app.code_view_mode, CodeViewMode::IrDiff);

        app.show_c_source_mode();
        assert_eq!(app.code_view_mode, CodeViewMode::CSource);

        app.rotate_code_view_mode_next();
        assert_eq!(app.code_view_mode, CodeViewMode::IrDiff);
    }

    #[test]
    fn side_by_side_diff_requires_analysis_step() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_function_select_modal();
        app.confirm_function_selection();

        app.toggle_side_by_side_diff();

        assert!(!app.is_side_by_side_diff_open());
        assert!(app.status_message.contains("Analysis results are required"));
    }

    #[test]
    fn side_by_side_diff_toggles_open_and_closed() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_function_select_modal();
        app.confirm_function_selection();

        let mut step = make_step(AnalysisStage::Vectorize, "loopvectorize", 0);
        step.ir_lines = vec![
            IrLine {
                tag: ChangeTag::Delete,
                text: String::from("old"),
                is_source_annotation: false,
                details: Default::default(),
            },
            IrLine {
                tag: ChangeTag::Insert,
                text: String::from("new"),
                is_source_annotation: false,
                details: Default::default(),
            },
        ];
        step.source_line_map = vec![None; step.ir_lines.len()];
        attach_ready_session(&mut app, step, vec![]);

        app.toggle_side_by_side_diff();
        assert!(app.is_side_by_side_diff_open());
        assert_eq!(app.side_by_side_diff_scroll, 0);

        app.toggle_side_by_side_diff();
        assert!(!app.is_side_by_side_diff_open());
        assert_eq!(app.side_by_side_diff_scroll, 0);
    }

    #[test]
    fn side_by_side_diff_scrolls_with_detail_navigation() {
        let mut app =
            AppState::new_with_run_mode(vec![benchmark("A")], FunctionRunMode::OutputFilter);
        app.open_function_select_modal();
        app.confirm_function_selection();

        let mut step = make_step(AnalysisStage::Vectorize, "loopvectorize", 0);
        step.ir_lines = (0..4)
            .map(|idx| IrLine {
                tag: ChangeTag::Equal,
                text: format!("line {idx}"),
                is_source_annotation: false,
                details: Default::default(),
            })
            .collect();
        step.source_line_map = vec![None; step.ir_lines.len()];
        attach_ready_session(&mut app, step, vec![]);

        app.toggle_side_by_side_diff();
        app.detail_move_down();
        app.detail_move_down();
        app.detail_move_up();

        assert!(app.is_side_by_side_diff_open());
        assert_eq!(app.side_by_side_diff_scroll, 1);
    }
}
