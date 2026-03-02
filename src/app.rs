use crate::model::{
    BenchmarkItem, CompileProfile, JobKind, LoopResult, RemarkEntry, RemarksSummary, RightTab,
    RunSession, SessionStatus,
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
    pub remarks_summary: RemarksSummary,
}

#[derive(Clone, Debug)]
pub enum JobState {
    Idle,
    Running(JobKind),
}

pub struct AppState {
    pub benchmarks: Vec<BenchmarkItem>,
    pub selected_idx: usize,
    pub active_profile: CompileProfile,
    pub active_tab: RightTab,
    pub current_session: Option<RunSession>,
    pub job_state: JobState,
    pub status_message: String,
}

impl AppState {
    pub fn new(benchmarks: Vec<BenchmarkItem>) -> Self {
        Self {
            benchmarks,
            selected_idx: 0,
            active_profile: CompileProfile::O3Remarks,
            active_tab: RightTab::Remarks,
            current_session: None,
            job_state: JobState::Idle,
            status_message: String::from("Ready"),
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

    pub fn switch_tab(&mut self) {
        self.active_tab = self.active_tab.next();
    }

    pub fn clear_session(&mut self) {
        self.current_session = None;
        self.status_message = String::from("Session cleared");
    }

    pub fn set_status_message(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
    }

    pub fn is_job_running(&self) -> bool {
        matches!(self.job_state, JobState::Running(_))
    }

    pub fn begin_job(&mut self, kind: JobKind, benchmark: String, profile: CompileProfile) {
        self.job_state = JobState::Running(kind);
        self.current_session = Some(RunSession::new_running(profile, benchmark.clone()));
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
                if self.current_session.is_none() {
                    self.current_session =
                        Some(RunSession::new_running(profile, benchmark.clone()));
                }
                self.status_message = format!("{kind} started for {benchmark} ({profile})");
            }
            JobEvent::LogLine(line) => {
                if let Some(session) = self.current_session.as_mut() {
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
                match result {
                    Ok(outcome) => {
                        let mut session = self.current_session.take().unwrap_or_else(|| {
                            RunSession::new_running(outcome.profile, outcome.benchmark.clone())
                        });
                        session.profile = outcome.profile;
                        session.benchmark = outcome.benchmark.clone();
                        session.loop_results = outcome.loop_results;
                        session.remarks = outcome.remarks;
                        session.remarks_summary = outcome.remarks_summary;
                        session.status = SessionStatus::Succeeded;
                        self.current_session = Some(session);
                        self.status_message =
                            format!("Completed: {} ({})", outcome.benchmark, outcome.profile);
                    }
                    Err(error) => {
                        if let Some(session) = self.current_session.as_mut() {
                            session.status = SessionStatus::Failed(error.clone());
                        }
                        self.status_message = format!("Job failed: {error}");
                    }
                }
            }
        }
    }
}
