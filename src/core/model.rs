use std::fmt;
use std::hash::{Hash, Hasher};

use similar::ChangeTag;

#[derive(Clone, Debug)]
pub struct IrLine {
    pub tag: ChangeTag,
    pub text: String,
    pub is_source_annotation: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbgLocation {
    pub line: u32,
    pub inlined_from: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BenchmarkItem {
    pub name: String,
    pub category: String,
    pub data_type: String,
    pub run_options: Vec<String>,
    pub available_functions: Vec<BenchmarkFunction>,
    pub source_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchmarkFunction {
    pub loop_id: String,
    pub symbol: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPage {
    BenchmarkList,
    BenchmarkDetail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptimizationLevel {
    O0,
    O1,
    O2,
    O3,
    Os,
    Oz,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildPurpose {
    Runtime,
    Analysis,
}

impl OptimizationLevel {
    pub fn flag(self) -> &'static str {
        match self {
            Self::O0 => "-O0",
            Self::O1 => "-O1",
            Self::O2 => "-O2",
            Self::O3 => "-O3",
            Self::Os => "-Os",
            Self::Oz => "-Oz",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::O0 => Self::O1,
            Self::O1 => Self::O2,
            Self::O2 => Self::O3,
            Self::O3 => Self::Os,
            Self::Os => Self::Oz,
            Self::Oz => Self::O0,
        }
    }
}

impl fmt::Display for OptimizationLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.flag())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ForceVectorWidth {
    Auto,
    W2,
    W4,
    W8,
    W16,
}

impl ForceVectorWidth {
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::W2,
            Self::W2 => Self::W4,
            Self::W4 => Self::W8,
            Self::W8 => Self::W16,
            Self::W16 => Self::Auto,
        }
    }

    pub fn flag(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::W2 => Some("-force-vector-width=2"),
            Self::W4 => Some("-force-vector-width=4"),
            Self::W8 => Some("-force-vector-width=8"),
            Self::W16 => Some("-force-vector-width=16"),
        }
    }
}

impl fmt::Display for ForceVectorWidth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Auto => "auto",
            Self::W2 => "2",
            Self::W4 => "4",
            Self::W8 => "8",
            Self::W16 => "16",
        };
        f.write_str(label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ForceInterleave {
    Auto,
    I1,
    I2,
    I4,
}

impl ForceInterleave {
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::I1,
            Self::I1 => Self::I2,
            Self::I2 => Self::I4,
            Self::I4 => Self::Auto,
        }
    }

    pub fn flag(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::I1 => Some("-force-vector-interleave=1"),
            Self::I2 => Some("-force-vector-interleave=2"),
            Self::I4 => Some("-force-vector-interleave=4"),
        }
    }
}

impl fmt::Display for ForceInterleave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Auto => "auto",
            Self::I1 => "1",
            Self::I2 => "2",
            Self::I4 => "4",
        };
        f.write_str(label)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CompilerConfig {
    pub opt_level: OptimizationLevel,
    pub enable_loop_vectorize: bool,
    pub enable_slp_vectorize: bool,
    pub fast_math: bool,
    pub march_native: bool,
    pub unroll_loops: bool,
    pub force_vector_width: ForceVectorWidth,
    pub force_vector_interleave: ForceInterleave,
    pub loop_interchange: bool,
    pub loop_distribute: bool,
    pub extra_c_flags: String,
    pub extra_llvm_flags: String,
}

impl Default for CompilerConfig {
    fn default() -> Self {
        Self {
            opt_level: OptimizationLevel::O3,
            enable_loop_vectorize: true,
            enable_slp_vectorize: true,
            fast_math: false,
            march_native: false,
            unroll_loops: true,
            force_vector_width: ForceVectorWidth::Auto,
            force_vector_interleave: ForceInterleave::Auto,
            loop_interchange: false,
            loop_distribute: false,
            extra_c_flags: String::new(),
            extra_llvm_flags: String::new(),
        }
    }
}

impl CompilerConfig {
    pub fn runtime_c_flags(&self) -> Vec<String> {
        let mut flags = vec![self.opt_level.flag().to_string()];
        if !self.enable_loop_vectorize {
            flags.push(String::from("-fno-vectorize"));
        }
        if !self.enable_slp_vectorize {
            flags.push(String::from("-fno-slp-vectorize"));
        }
        if self.fast_math {
            flags.push(String::from("-ffast-math"));
        }
        if self.march_native {
            flags.push(String::from("-march=native"));
        }
        if !self.unroll_loops {
            flags.push(String::from("-fno-unroll-loops"));
        }
        if let Some(f) = self.force_vector_width.flag() {
            flags.push(String::from("-mllvm"));
            flags.push(String::from(f));
        }
        if let Some(f) = self.force_vector_interleave.flag() {
            flags.push(String::from("-mllvm"));
            flags.push(String::from(f));
        }
        if self.loop_interchange {
            flags.push(String::from("-mllvm"));
            flags.push(String::from("-enable-loopinterchange"));
        }
        if self.loop_distribute {
            flags.push(String::from("-mllvm"));
            flags.push(String::from("-enable-loop-distribute"));
        }
        flags.extend(split_flags(&self.extra_c_flags));
        flags
    }

    pub fn analysis_c_flags(&self) -> Vec<String> {
        let mut flags = self.runtime_c_flags();
        flags.push(String::from("-g"));
        flags.push(String::from("-Rpass=loop-vectorize"));
        flags.push(String::from("-Rpass-missed=loop-vectorize"));
        flags.push(String::from("-Rpass-analysis=loop-vectorize"));
        flags.push(String::from("-fsave-optimization-record"));
        flags.push(String::from("-mllvm"));
        flags.push(String::from("-print-changed"));
        flags.push(String::from("-mllvm"));
        flags.push(String::from("-print-module-scope"));

        for token in split_flags(&self.extra_llvm_flags) {
            flags.push(String::from("-mllvm"));
            flags.push(token);
        }

        flags
    }

    pub fn c_flags_for(&self, purpose: BuildPurpose) -> Vec<String> {
        match purpose {
            BuildPurpose::Runtime => self.runtime_c_flags(),
            BuildPurpose::Analysis => self.analysis_c_flags(),
        }
    }

    pub fn label(&self) -> String {
        format!(
            "{} lv:{} slp:{} fm:{}",
            self.opt_level,
            on_off(self.enable_loop_vectorize),
            on_off(self.enable_slp_vectorize),
            on_off(self.fast_math),
        )
    }

    pub fn canonical_key(&self) -> String {
        format!(
            "opt={}|lv={}|slp={}|fm={}|march={}|unroll={}|vw={}|vi={}|interchange={}|distribute={}|extra_c={}|extra_llvm={}",
            self.opt_level.flag(),
            self.enable_loop_vectorize as u8,
            self.enable_slp_vectorize as u8,
            self.fast_math as u8,
            self.march_native as u8,
            self.unroll_loops as u8,
            self.force_vector_width,
            self.force_vector_interleave,
            self.loop_interchange as u8,
            self.loop_distribute as u8,
            self.extra_c_flags.trim(),
            self.extra_llvm_flags.trim(),
        )
    }

    pub fn config_id(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.canonical_key().hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

impl fmt::Display for CompilerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

fn split_flags(text: &str) -> Vec<String> {
    text.split_whitespace().map(ToString::to_string).collect()
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    BuildAndRun,
    AnalyzeFast,
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::BuildAndRun => "Build+Run",
            Self::AnalyzeFast => "Analyze",
        };
        write!(f, "{text}")
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct LoopResult {
    pub loop_id: String,
    pub time_sec: f64,
    pub checksum: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemarkKind {
    Passed,
    Missed,
    Analysis,
    Other,
}

impl fmt::Display for RemarkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Passed => "Passed",
            Self::Missed => "Missed",
            Self::Analysis => "Analysis",
            Self::Other => "Other",
        };
        write!(f, "{text}")
    }
}

#[derive(Clone, Debug)]
pub struct RemarkEntry {
    pub kind: RemarkKind,
    pub pass: String,
    pub name: String,
    #[allow(dead_code)]
    pub file: Option<String>,
    #[allow(dead_code)]
    pub line: Option<u32>,
    pub function: Option<String>,
    pub message: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct OptimizationStep {
    pub pass: String,
    pub total: usize,
    pub passed: usize,
    pub missed: usize,
    pub analysis: usize,
    pub other: usize,
    pub remark_indices: Vec<usize>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct IrDiffStep {
    pub index: usize,
    pub pass: String,
    pub target: String,
    pub changed_lines: usize,
    pub diff_text: String,
    pub remark_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnalysisStage {
    Initial,
    Interprocedural,
    Loop,
    Vectorize,
    Cleanup,
    Other,
}

impl AnalysisStage {
    pub fn pipeline_order(self) -> u8 {
        match self {
            Self::Initial => 0,
            Self::Interprocedural => 1,
            Self::Loop => 2,
            Self::Vectorize => 3,
            Self::Cleanup => 4,
            Self::Other => 5,
        }
    }

    pub fn ui_label(self) -> &'static str {
        match self {
            Self::Initial => "Initial",
            Self::Interprocedural => "Interproc",
            Self::Loop => "Loop Opts",
            Self::Vectorize => "Vectorize",
            Self::Cleanup => "Cleanup",
            Self::Other => "Other",
        }
    }
}

impl fmt::Display for AnalysisStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ui_label())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisSource {
    TraceFast,
}

impl fmt::Display for AnalysisSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::TraceFast => "trace",
        };
        write!(f, "{label}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisState {
    None,
    Running,
    Ready,
    Failed,
}

impl fmt::Display for AnalysisState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::None => "none",
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Failed => "failed",
        };
        write!(f, "{label}")
    }
}

#[derive(Clone, Debug)]
pub struct AnalysisStep {
    #[allow(dead_code)]
    pub visible_index: usize,
    #[allow(dead_code)]
    pub raw_index: usize,
    #[allow(dead_code)]
    pub pass: String,
    pub pass_key: String,
    #[allow(dead_code)]
    pub pass_occurrence: usize,
    pub stage: AnalysisStage,
    #[allow(dead_code)]
    pub target_raw: String,
    #[allow(dead_code)]
    pub target_function: Option<String>,
    pub changed_lines: usize,
    pub diff_text: String,
    pub ir_lines: Vec<IrLine>,
    pub source_line_map: Vec<Option<u32>>,
    pub remark_indices: Vec<usize>,
    #[allow(dead_code)]
    pub source: AnalysisSource,
}

#[derive(Default, Clone, Debug)]
pub struct RemarksSummary {
    pub total_loop_vectorize: usize,
    pub vectorized: usize,
    pub missed_details: usize,
    pub not_beneficial: usize,
}

impl RemarksSummary {
    pub fn from_entries(entries: &[RemarkEntry]) -> Self {
        let mut summary = Self::default();
        for entry in entries {
            if entry.pass != "loop-vectorize" {
                continue;
            }
            summary.total_loop_vectorize += 1;
            match entry.name.as_str() {
                "Vectorized" => summary.vectorized += 1,
                "MissedDetails" => summary.missed_details += 1,
                "VectorizationNotBeneficial" => summary.not_beneficial += 1,
                _ => {}
            }
        }
        summary
    }
}

#[derive(Clone, Debug)]
pub enum SessionStatus {
    Running,
    Succeeded,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionRunMode {
    RealSelective,
    OutputFilter,
}

impl fmt::Display for FunctionRunMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::RealSelective => "real-selective",
            Self::OutputFilter => "output-filter",
        };
        write!(f, "{text}")
    }
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Succeeded => write!(f, "succeeded"),
            Self::Failed(reason) => write!(f, "failed: {reason}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RunSession {
    pub compiler_config: CompilerConfig,
    pub config_id: String,
    pub benchmark: String,
    pub selected_function_loop_id: String,
    pub selected_function_symbol: String,
    pub run_mode: FunctionRunMode,
    pub loop_results: Vec<LoopResult>,
    pub remarks: Vec<RemarkEntry>,
    pub analysis_steps: Vec<AnalysisStep>,
    pub analysis_state: AnalysisState,
    pub remarks_summary: RemarksSummary,
    pub logs: Vec<String>,
    pub status: SessionStatus,
}

impl RunSession {
    pub fn new_running(
        compiler_config: CompilerConfig,
        benchmark: String,
        selected_function_loop_id: String,
        selected_function_symbol: String,
        run_mode: FunctionRunMode,
    ) -> Self {
        let config_id = compiler_config.config_id();
        Self {
            compiler_config,
            config_id,
            benchmark,
            selected_function_loop_id,
            selected_function_symbol,
            run_mode,
            loop_results: Vec::new(),
            remarks: Vec::new(),
            analysis_steps: Vec::new(),
            analysis_state: AnalysisState::None,
            remarks_summary: RemarksSummary::default(),
            logs: Vec::new(),
            status: SessionStatus::Running,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_compiler_config_has_analysis_features_enabled() {
        let cfg = CompilerConfig::default();
        let flags = cfg.analysis_c_flags();
        assert!(flags.iter().any(|f| f == "-g"));
        assert!(flags.iter().any(|f| f == "-fsave-optimization-record"));
        assert!(flags.iter().any(|f| f == "-print-changed"));
        assert!(flags.iter().any(|f| f == "-print-module-scope"));
        assert!(flags.iter().any(|f| f == "-Rpass=loop-vectorize"));
        assert!(flags.iter().any(|f| f == "-Rpass-missed=loop-vectorize"));
        assert!(flags.iter().any(|f| f == "-Rpass-analysis=loop-vectorize"));
    }

    #[test]
    fn runtime_flags_reflect_vectorizer_toggles() {
        let cfg = CompilerConfig {
            enable_loop_vectorize: false,
            enable_slp_vectorize: false,
            ..CompilerConfig::default()
        };
        let flags = cfg.runtime_c_flags();
        assert!(flags.iter().any(|f| f == "-fno-vectorize"));
        assert!(flags.iter().any(|f| f == "-fno-slp-vectorize"));
    }

    #[test]
    fn config_id_changes_when_field_changes() {
        let cfg_a = CompilerConfig::default();
        let cfg_b = CompilerConfig {
            fast_math: true,
            ..CompilerConfig::default()
        };
        assert_ne!(cfg_a.config_id(), cfg_b.config_id());
    }

    #[test]
    fn runtime_flags_include_fast_math() {
        let cfg = CompilerConfig {
            fast_math: true,
            ..CompilerConfig::default()
        };
        let flags = cfg.runtime_c_flags();
        assert!(flags.iter().any(|f| f == "-ffast-math"));
    }

    #[test]
    fn runtime_flags_include_force_vector_width() {
        let cfg = CompilerConfig {
            force_vector_width: ForceVectorWidth::W4,
            ..CompilerConfig::default()
        };
        let flags = cfg.runtime_c_flags();
        assert!(flags.iter().any(|f| f == "-force-vector-width=4"));
    }

    #[test]
    fn runtime_flags_no_unroll_when_disabled() {
        let cfg = CompilerConfig {
            unroll_loops: false,
            ..CompilerConfig::default()
        };
        let flags = cfg.runtime_c_flags();
        assert!(flags.iter().any(|f| f == "-fno-unroll-loops"));
    }

    #[test]
    fn runtime_flags_include_loop_interchange() {
        let cfg = CompilerConfig {
            loop_interchange: true,
            ..CompilerConfig::default()
        };
        let flags = cfg.runtime_c_flags();
        assert!(flags.iter().any(|f| f == "-enable-loopinterchange"));
    }

    #[test]
    fn runtime_flags_include_loop_distribute() {
        let cfg = CompilerConfig {
            loop_distribute: true,
            ..CompilerConfig::default()
        };
        let flags = cfg.runtime_c_flags();
        assert!(flags.iter().any(|f| f == "-enable-loop-distribute"));
    }
}
