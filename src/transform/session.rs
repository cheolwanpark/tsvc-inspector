use std::fmt::Write;

use similar::ChangeTag;

use crate::core::model::{
    AnalysisStage, AnalysisStep, BenchmarkFunction, BenchmarkItem, RemarkEntry, RunSession,
};

pub struct DetailSnapshotInput<'a> {
    pub benchmark: &'a BenchmarkItem,
    pub selected_function: &'a BenchmarkFunction,
    pub session: &'a RunSession,
    pub selected_stage: AnalysisStage,
    pub detail_focus_label: &'a str,
    pub step: &'a AnalysisStep,
    pub selected_pass_index: usize,
    pub passes_len: usize,
    pub source_text: &'a str,
}

pub fn build_detail_snapshot(input: DetailSnapshotInput<'_>) -> String {
    let full_pass_diff = build_full_pass_diff(input.step);
    let pass_name = if input.step.pass.is_empty() {
        input.step.pass_key.as_str()
    } else {
        input.step.pass.as_str()
    };
    let remarks_text = format_step_remarks(input.step, &input.session.remarks);

    let mut out = String::new();
    let _ = writeln!(out, "TSVC Detail Snapshot");
    let _ = writeln!(out);

    let _ = writeln!(out, "Context");
    let _ = writeln!(out, "- benchmark: {}", input.benchmark.name);
    let _ = writeln!(
        out,
        "- function: {} ({})",
        input.selected_function.loop_id, input.selected_function.symbol
    );
    let _ = writeln!(out, "- config: {}", input.session.compiler_config);
    let _ = writeln!(out, "- config_id: {}", input.session.config_id);
    let _ = writeln!(out, "- focus: {}", input.detail_focus_label);
    let _ = writeln!(out, "- analysis_state: {}", input.session.analysis_state);
    let _ = writeln!(out);

    let _ = writeln!(out, "Stage/Pass");
    let _ = writeln!(out, "- stage: {}", input.selected_stage.ui_label());
    let _ = writeln!(out, "- pass_key: {}", input.step.pass_key);
    let _ = writeln!(out, "- pass_raw: {}", input.step.pass);
    let _ = writeln!(
        out,
        "- pass_index: {}/{}",
        input.selected_pass_index, input.passes_len
    );
    let _ = writeln!(out, "- changed_lines: {}", input.step.changed_lines);
    let _ = writeln!(out);

    let _ = writeln!(out, "Remarks");
    let _ = writeln!(out, "{remarks_text}");
    let _ = writeln!(out);

    let _ = writeln!(out, "C Source");
    let _ = writeln!(out, "```c");
    let _ = writeln!(out, "{}", input.source_text);
    let _ = writeln!(out, "```");
    let _ = writeln!(out);

    let _ = writeln!(out, "IR Diff ({pass_name})");
    let _ = writeln!(out, "```diff");
    let _ = writeln!(out, "{full_pass_diff}");
    let _ = writeln!(out, "```");

    out
}

pub fn has_vectorizer_ir_changes(session: &RunSession) -> bool {
    session.analysis_steps.iter().any(|step| {
        step.stage == AnalysisStage::Vectorize
            && step.changed_lines > 0
            && matches!(step.pass_key.as_str(), "loopvectorize" | "slpvectorizer")
    })
}

pub fn extract_vf_from_remarks(remarks: &[RemarkEntry]) -> Option<u32> {
    for r in remarks {
        if r.pass == "loop-vectorize"
            && let Some(msg) = &r.message
        {
            for pattern in &["VF = ", "VF="] {
                if let Some(pos) = msg.find(pattern) {
                    let rest = msg[pos + pattern.len()..].trim_start_matches(' ');
                    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = num.parse::<u32>() {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

fn format_step_remarks(step: &AnalysisStep, remarks: &[RemarkEntry]) -> String {
    let mut lines = Vec::new();
    for idx in &step.remark_indices {
        let Some(remark) = remarks.get(*idx) else {
            continue;
        };
        let message = remark.message.as_deref().unwrap_or(remark.name.as_str());
        let line = format!(
            "- [{}] {}::{} {}",
            remark.kind, remark.pass, remark.name, message
        );
        lines.push(line);
    }
    if lines.is_empty() {
        return String::from("- (none)");
    }
    lines.join("\n")
}

fn build_full_pass_diff(step: &AnalysisStep) -> String {
    if step.ir_lines.is_empty() {
        return step.diff_text.clone();
    }
    step.ir_lines
        .iter()
        .map(|line| {
            if line.is_source_annotation {
                return line.text.clone();
            }
            let prefix = match line.tag {
                ChangeTag::Insert => "+ ",
                ChangeTag::Delete => "- ",
                ChangeTag::Equal => "  ",
            };
            format!("{prefix}{}", line.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
