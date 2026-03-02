use std::fmt;

#[derive(Clone, Debug)]
pub struct BenchmarkItem {
    pub name: String,
    pub category: String,
    pub data_type: String,
    pub run_options: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPage {
    BenchmarkList,
    BenchmarkDetail,
}

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompileProfile {
    O3Remarks,
    O3NoVec,
    O3Default,
}

impl CompileProfile {
    pub fn c_flags(self) -> &'static str {
        match self {
            Self::O3Remarks => {
                "-O3 -Rpass=loop-vectorize -Rpass-missed=loop-vectorize -Rpass-analysis=loop-vectorize -fsave-optimization-record"
            }
            Self::O3NoVec => {
                "-O3 -fno-vectorize -fno-slp-vectorize -Rpass-missed=loop-vectorize -Rpass-analysis=loop-vectorize -fsave-optimization-record"
            }
            Self::O3Default => "-O3 -fsave-optimization-record",
        }
    }

    pub fn build_dir_name(self) -> &'static str {
        match self {
            Self::O3Remarks => "build-tsvc-o3-remarks",
            Self::O3NoVec => "build-tsvc-o3-novec",
            Self::O3Default => "build-tsvc-o3-default",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::O3Remarks => "O3 + remarks",
            Self::O3NoVec => "O3 no-vectorize",
            Self::O3Default => "O3 default",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::O3Remarks => Self::O3NoVec,
            Self::O3NoVec => Self::O3Default,
            Self::O3Default => Self::O3Remarks,
        }
    }
}

impl fmt::Display for CompileProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Build,
    Run,
    BuildAndRun,
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Build => "Build",
            Self::Run => "Run",
            Self::BuildAndRun => "Build+Run",
        };
        write!(f, "{text}")
    }
}

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
    pub file: Option<String>,
    pub line: Option<u32>,
    pub function: Option<String>,
    pub message: Option<String>,
}

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
    pub profile: CompileProfile,
    pub benchmark: String,
    pub loop_results: Vec<LoopResult>,
    pub remarks: Vec<RemarkEntry>,
    pub optimization_steps: Vec<OptimizationStep>,
    pub remarks_summary: RemarksSummary,
    pub logs: Vec<String>,
    pub status: SessionStatus,
}

impl RunSession {
    pub fn new_running(profile: CompileProfile, benchmark: String) -> Self {
        Self {
            profile,
            benchmark,
            loop_results: Vec::new(),
            remarks: Vec::new(),
            optimization_steps: Vec::new(),
            remarks_summary: RemarksSummary::default(),
            logs: Vec::new(),
            status: SessionStatus::Running,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightTab {
    StepDetails,
    BuildLog,
}

impl RightTab {
    pub fn next(self) -> Self {
        match self {
            Self::StepDetails => Self::BuildLog,
            Self::BuildLog => Self::StepDetails,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::StepDetails => 0,
            Self::BuildLog => 1,
        }
    }
}
