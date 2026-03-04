use std::collections::HashMap;
use std::fs;
use std::path::Path;

use regex::Regex;
use similar::{ChangeTag, TextDiff};

use crate::error::AppResult;
use crate::model::{
    AnalysisSource, AnalysisStage, AnalysisStep, IrDiffStep, IrLine, LoopResult,
    OptimizationStep, RemarkEntry, RemarkKind,
};

#[allow(dead_code)]
const MAX_IR_STEPS: usize = 500;
const MAX_DIFF_LINES: usize = 8000;

#[derive(Clone, Debug)]
pub struct IrSnapshot {
    pub raw_index: usize,
    pub pass: String,
    pub pass_occurrence: usize,
    pub target: String,
    pub snapshot: String,
}

pub fn parse_tsvc_stdout(stdout: &str) -> Vec<LoopResult> {
    let row_re = Regex::new(r"^\s*([A-Za-z0-9]+)\s+([0-9]+(?:\.[0-9]+)?)\s+(.+?)\s*$")
        .expect("valid TSVC output row regex");
    let mut in_table = false;
    let mut rows = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Loop") && trimmed.contains("Time") {
            in_table = true;
            continue;
        }
        if !in_table || trimmed.is_empty() {
            continue;
        }

        if let Some(caps) = row_re.captures(trimmed) {
            let loop_id = caps
                .get(1)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let time_sec = caps
                .get(2)
                .and_then(|m| m.as_str().parse::<f64>().ok())
                .unwrap_or(0.0);
            let checksum = caps
                .get(3)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            rows.push(LoopResult {
                loop_id,
                time_sec,
                checksum,
            });
        }
    }

    rows
}

pub fn parse_opt_remarks(path: &Path) -> AppResult<Vec<RemarkEntry>> {
    let content = fs::read_to_string(path)?;
    let mut blocks = Vec::new();
    let mut current = Vec::new();

    for line in content.lines() {
        if line.starts_with("--- !") && !current.is_empty() {
            blocks.push(current.join("\n"));
            current.clear();
        }
        current.push(line.to_string());
        if line.trim() == "..." {
            blocks.push(current.join("\n"));
            current.clear();
        }
    }
    if !current.is_empty() {
        blocks.push(current.join("\n"));
    }

    let file_line_re = Regex::new(r"File:\s*'([^']+)'.*Line:\s*([0-9]+)")?;
    let message_re = Regex::new(r"String:\s*(.+)$")?;

    let mut entries = Vec::new();
    for block in blocks {
        let lines = block.lines().collect::<Vec<_>>();
        if lines.is_empty() {
            continue;
        }

        let kind = parse_kind(lines[0]);
        let mut pass: Option<String> = None;
        let mut name: Option<String> = None;
        let mut file: Option<String> = None;
        let mut line: Option<u32> = None;
        let mut function: Option<String> = None;
        let mut message: Option<String> = None;

        for raw_line in &lines {
            let l = raw_line.trim();
            if let Some(v) = l.strip_prefix("Pass:") {
                pass = Some(v.trim().to_string());
                continue;
            }
            if let Some(v) = l.strip_prefix("Name:") {
                name = Some(v.trim().to_string());
                continue;
            }
            if let Some(v) = l.strip_prefix("Function:") {
                function = Some(v.trim().to_string());
                continue;
            }
            if file.is_none()
                && let Some(caps) = file_line_re.captures(l)
            {
                file = caps.get(1).map(|m| m.as_str().to_string());
                line = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
                continue;
            }
            if message.is_none()
                && let Some(caps) = message_re.captures(l)
            {
                let mut text = caps
                    .get(1)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default();
                if text.starts_with('\'') && text.ends_with('\'') && text.len() >= 2 {
                    text = text[1..text.len() - 1].to_string();
                }
                message = Some(text);
            }
        }

        entries.push(RemarkEntry {
            kind,
            pass: pass.unwrap_or_default(),
            name: name.unwrap_or_default(),
            file,
            line,
            function,
            message,
        });
    }

    Ok(entries)
}

#[allow(dead_code)]
pub fn group_optimization_steps(entries: &[RemarkEntry]) -> Vec<OptimizationStep> {
    let mut steps = Vec::new();
    let mut step_by_pass = HashMap::<String, usize>::new();

    for (remark_idx, entry) in entries.iter().enumerate() {
        let pass_name = if entry.pass.trim().is_empty() {
            String::from("(unknown-pass)")
        } else {
            entry.pass.clone()
        };

        let step_idx = if let Some(idx) = step_by_pass.get(&pass_name).copied() {
            idx
        } else {
            let idx = steps.len();
            steps.push(OptimizationStep {
                pass: pass_name.clone(),
                total: 0,
                passed: 0,
                missed: 0,
                analysis: 0,
                other: 0,
                remark_indices: Vec::new(),
            });
            step_by_pass.insert(pass_name, idx);
            idx
        };

        let step = &mut steps[step_idx];
        step.total += 1;
        match entry.kind {
            RemarkKind::Passed => step.passed += 1,
            RemarkKind::Missed => step.missed += 1,
            RemarkKind::Analysis => step.analysis += 1,
            RemarkKind::Other => step.other += 1,
        }
        step.remark_indices.push(remark_idx);
    }

    steps
}

pub fn parse_ir_snapshots_from_trace(build_trace: &str) -> Vec<IrSnapshot> {
    let header_re = Regex::new(r"^\*\*\* IR Dump (At Start|After .+) \*\*\*$")
        .expect("valid ir dump header regex");
    let start_re =
        Regex::new(r"^\*\*\* IR Dump At Start \*\*\*$").expect("valid ir dump start regex");
    let after_re = Regex::new(r"^\*\*\* IR Dump After (.+?) on (.+?) \*\*\*$")
        .expect("valid ir dump after regex");
    let no_change_re = Regex::new(r"^\*\*\* IR Dump After .+ omitted because no change \*\*\*$")
        .expect("valid ir no-change regex");

    let mut i = 0usize;
    let lines = build_trace.lines().collect::<Vec<_>>();
    let mut raw_after_index = 0usize;
    let mut pass_occurrence_by_key = HashMap::<String, usize>::new();
    let mut snapshots = Vec::new();

    while i < lines.len() {
        let header = lines[i].trim();
        if !header_re.is_match(header) {
            i += 1;
            continue;
        }
        i += 1;

        if start_re.is_match(header) {
            let mut body = Vec::new();
            while i < lines.len() && !header_re.is_match(lines[i].trim()) {
                body.push(lines[i]);
                i += 1;
            }
            snapshots.push(IrSnapshot {
                raw_index: 0,
                pass: String::from("(initial IR)"),
                pass_occurrence: 1,
                target: String::from("[module]"),
                snapshot: body.join("\n"),
            });
            continue;
        }

        let Some(caps) = after_re.captures(header) else {
            continue;
        };

        raw_after_index = raw_after_index.saturating_add(1);
        let pass = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| String::from("(unknown-pass)"));
        let target = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| String::from("(unknown-target)"));
        let pass_key = normalize_pass_key(&pass);
        let pass_occurrence = {
            let next = pass_occurrence_by_key.get(&pass_key).copied().unwrap_or(0) + 1;
            pass_occurrence_by_key.insert(pass_key, next);
            next
        };

        if no_change_re.is_match(header) {
            continue;
        }

        let mut body = Vec::new();
        while i < lines.len() && !header_re.is_match(lines[i].trim()) {
            body.push(lines[i]);
            i += 1;
        }

        snapshots.push(IrSnapshot {
            raw_index: raw_after_index,
            pass,
            pass_occurrence,
            target,
            snapshot: body.join("\n"),
        });
    }

    snapshots
}

pub fn build_fast_analysis_steps(
    build_trace: &str,
    selected_function_symbol: &str,
    remarks: &[RemarkEntry],
) -> Vec<AnalysisStep> {
    let snapshots = parse_ir_snapshots_from_trace(build_trace);
    build_analysis_steps_from_snapshots(
        &snapshots,
        selected_function_symbol,
        remarks,
        AnalysisSource::TraceFast,
    )
}

/// Scans module IR for `!N = !DILocation(line: X, ...)` entries.
/// Returns metadata ID → source line number.
pub fn parse_dbg_locations(module_ir: &str) -> HashMap<u32, u32> {
    let re = Regex::new(r"!(\d+) = !DILocation\(line: (\d+)").expect("valid DILocation regex");
    let mut map = HashMap::new();
    for caps in re.captures_iter(module_ir) {
        if let (Some(id_match), Some(line_match)) = (caps.get(1), caps.get(2)) {
            if let (Ok(id), Ok(line)) = (
                id_match.as_str().parse::<u32>(),
                line_match.as_str().parse::<u32>(),
            ) {
                map.insert(id, line);
            }
        }
    }
    map
}

/// For each IrLine, extracts `!dbg !N` and looks up the source line number.
pub fn build_source_line_map(
    ir_lines: &[IrLine],
    dbg_locations: &HashMap<u32, u32>,
) -> Vec<Option<u32>> {
    let dbg_re = Regex::new(r"!dbg !(\d+)").expect("valid dbg ref regex");
    ir_lines
        .iter()
        .map(|ir_line| {
            let caps = dbg_re.captures(&ir_line.text)?;
            let id: u32 = caps.get(1)?.as_str().parse().ok()?;
            dbg_locations.get(&id).copied()
        })
        .collect()
}

pub fn build_analysis_steps_from_snapshots(
    snapshots: &[IrSnapshot],
    selected_function_symbol: &str,
    remarks: &[RemarkEntry],
    source: AnalysisSource,
) -> Vec<AnalysisStep> {
    let mut out = Vec::new();
    let mut prev_ir: Option<String> = None;
    let mut prev_raw_index = 0usize;

    for snapshot in snapshots {
        let Some(function_ir) = extract_function_ir(&snapshot.snapshot, selected_function_symbol)
        else {
            continue;
        };

        let target_function = target_function_from_label(&snapshot.target);
        let stage = classify_analysis_stage(snapshot.raw_index, &snapshot.pass, &snapshot.target);
        let remark_indices =
            collect_analysis_remark_indices(&snapshot.pass, target_function.as_deref(), remarks);

        let dbg_locations = parse_dbg_locations(&snapshot.snapshot);

        if prev_ir.is_none() {
            let ir_lines: Vec<IrLine> = function_ir
                .lines()
                .map(|l| IrLine {
                    tag: ChangeTag::Equal,
                    text: l.to_string(),
                })
                .collect();
            let source_line_map = build_source_line_map(&ir_lines, &dbg_locations);
            out.push(AnalysisStep {
                visible_index: out.len(),
                raw_index: snapshot.raw_index,
                pass: snapshot.pass.clone(),
                pass_key: normalize_pass_key(&snapshot.pass),
                pass_occurrence: snapshot.pass_occurrence.max(1),
                stage,
                target_raw: snapshot.target.clone(),
                target_function,
                changed_lines: 0,
                diff_text: String::from("No previous step. This is the initial IR snapshot."),
                ir_lines,
                source_line_map,
                remark_indices,
                source,
            });
            prev_raw_index = snapshot.raw_index;
            prev_ir = Some(function_ir);
            continue;
        }

        let Some(before) = prev_ir.as_ref() else {
            continue;
        };
        if before == &function_ir {
            continue;
        }

        let diff = TextDiff::from_lines(before, &function_ir);
        let changed_lines = diff
            .iter_all_changes()
            .filter(|change| change.tag() != ChangeTag::Equal)
            .count();

        let ir_lines: Vec<IrLine> = diff
            .iter_all_changes()
            .map(|change| IrLine {
                tag: change.tag(),
                text: change.value().trim_end_matches('\n').to_string(),
            })
            .collect();
        let source_line_map = build_source_line_map(&ir_lines, &dbg_locations);

        let mut diff_text = diff
            .unified_diff()
            .context_radius(3)
            .header(
                &format!("raw-{prev_raw_index:05}"),
                &format!("raw-{:05}", snapshot.raw_index),
            )
            .to_string();
        if diff_text.trim().is_empty() {
            diff_text = String::from("No textual diff was produced for this step.");
        } else {
            diff_text = truncate_lines(&diff_text, MAX_DIFF_LINES);
        }

        out.push(AnalysisStep {
            visible_index: out.len(),
            raw_index: snapshot.raw_index,
            pass: snapshot.pass.clone(),
            pass_key: normalize_pass_key(&snapshot.pass),
            pass_occurrence: snapshot.pass_occurrence.max(1),
            stage,
            target_raw: snapshot.target.clone(),
            target_function,
            changed_lines,
            diff_text,
            ir_lines,
            source_line_map,
            remark_indices,
            source,
        });

        prev_raw_index = snapshot.raw_index;
        prev_ir = Some(function_ir);
    }

    out
}

#[allow(dead_code)]
pub fn parse_ir_diff_steps(build_trace: &str, remarks: &[RemarkEntry]) -> Vec<IrDiffStep> {
    let header_re = Regex::new(r"^\*\*\* IR Dump (At Start|After .+) \*\*\*$")
        .expect("valid ir dump header regex");
    let start_re =
        Regex::new(r"^\*\*\* IR Dump At Start \*\*\*$").expect("valid ir dump start regex");
    let after_re = Regex::new(r"^\*\*\* IR Dump After (.+?) on (.+?) \*\*\*$")
        .expect("valid ir dump after regex");
    let no_change_re = Regex::new(r"^\*\*\* IR Dump After .+ omitted because no change \*\*\*$")
        .expect("valid ir no-change regex");

    let mut i = 0usize;
    let lines = build_trace.lines().collect::<Vec<_>>();
    let mut prev_snapshot: Option<String> = None;
    let mut steps = Vec::new();

    while i < lines.len() {
        let header = lines[i].trim();
        if !header_re.is_match(header) {
            i += 1;
            continue;
        }
        i += 1;

        if no_change_re.is_match(header) {
            continue;
        }

        let mut body = Vec::new();
        while i < lines.len() && !header_re.is_match(lines[i].trim()) {
            body.push(lines[i]);
            i += 1;
        }
        let snapshot = body.join("\n");

        if start_re.is_match(header) {
            prev_snapshot = Some(snapshot);
            continue;
        }

        let Some(caps) = after_re.captures(header) else {
            continue;
        };
        let pass = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| String::from("(unknown-pass)"));
        let target = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| String::from("(unknown-target)"));

        let Some(before) = prev_snapshot.as_ref() else {
            prev_snapshot = Some(snapshot);
            continue;
        };
        if steps.len() >= MAX_IR_STEPS {
            break;
        }

        let diff = TextDiff::from_lines(before, &snapshot);
        let changed_lines = diff
            .iter_all_changes()
            .filter(|change| change.tag() != ChangeTag::Equal)
            .count();
        let mut diff_text = diff
            .unified_diff()
            .context_radius(3)
            .header("before", "after")
            .to_string();
        diff_text = truncate_lines(&diff_text, MAX_DIFF_LINES);

        steps.push(IrDiffStep {
            index: steps.len() + 1,
            remark_indices: collect_step_remark_indices(&pass, &target, remarks),
            pass,
            target,
            changed_lines,
            diff_text,
        });

        prev_snapshot = Some(snapshot);
    }

    steps
}

fn collect_analysis_remark_indices(
    pass: &str,
    target_function: Option<&str>,
    remarks: &[RemarkEntry],
) -> Vec<usize> {
    let pass_key = normalize_pass_key(pass);
    let matching = remarks
        .iter()
        .enumerate()
        .filter(|(_, remark)| normalize_pass_key(&remark.pass) == pass_key)
        .collect::<Vec<_>>();

    let Some(target_function) = target_function else {
        return matching.into_iter().map(|(idx, _)| idx).collect();
    };

    let exact = matching
        .iter()
        .filter_map(|(idx, remark)| {
            if remark
                .function
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case(target_function))
            {
                Some(*idx)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if !exact.is_empty() {
        return exact;
    }

    matching.into_iter().map(|(idx, _)| idx).collect()
}

fn classify_analysis_stage(raw_index: usize, pass: &str, target: &str) -> AnalysisStage {
    if raw_index == 0 {
        return AnalysisStage::Initial;
    }

    let pass_key = normalize_pass_key(pass);
    let target_lower = target.to_ascii_lowercase();

    let vector_keywords = [
        "loopvectorize",
        "slpvectorizer",
        "vectorcombine",
        "looploadelimination",
        "looploadelim",
        "inferalignment",
        "injecttlimappings",
        "vectorize",
    ];
    if pass_contains_any(&pass_key, &vector_keywords) {
        return AnalysisStage::Vectorize;
    }

    let loop_keywords = [
        "loop",
        "lcssa",
        "licm",
        "indvar",
        "unswitch",
        "loopsink",
        "loopunroll",
        "loopsimplify",
        "loopdeletion",
        "loopdistribute",
        "loopinterchange",
        "loopidiom",
        "loopflatten",
        "loopfuse",
        "loopinstsimplify",
        "loopsimplifycfg",
        "loopversioning",
    ];
    if target_lower.starts_with("loop ") || pass_contains_any(&pass_key, &loop_keywords) {
        return AnalysisStage::Loop;
    }

    let interprocedural_keywords = [
        "inline",
        "ipsccp",
        "openmpopt",
        "calledvaluepropagation",
        "globalopt",
        "globaldce",
        "deadargument",
        "functionattrs",
        "attributor",
        "cgprofile",
        "constmerge",
        "elimavailextern",
        "recomputeglobalsaa",
        "rpofunctionattrs",
        "rellookuptableconverter",
        "annotation2metadata",
        "forceattrs",
        "inferattrs",
        "memprofremoveattributes",
        "devirt",
        "moduleinliner",
        "partialinliner",
    ];
    let is_interproc_target =
        target == "[module]" || (target.starts_with('(') && target.ends_with(')'));
    if is_interproc_target || pass_contains_any(&pass_key, &interprocedural_keywords) {
        return AnalysisStage::Interprocedural;
    }

    if pass_key.is_empty() {
        return AnalysisStage::Other;
    }

    AnalysisStage::Cleanup
}

fn pass_contains_any(pass_key: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| pass_key.contains(keyword))
}

fn target_function_from_label(target: &str) -> Option<String> {
    let trimmed = target.trim();
    if trimmed.is_empty() || trimmed == "[module]" {
        return None;
    }
    if let Some(inner) = trimmed
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        && looks_like_symbol(inner)
    {
        return Some(inner.to_string());
    }
    if let Some((_, function_name)) = trimmed.rsplit_once(" in function ")
        && looks_like_symbol(function_name)
    {
        return Some(function_name.to_string());
    }
    if looks_like_symbol(trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

fn looks_like_symbol(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
}

fn extract_function_ir(snapshot: &str, selected_symbol: &str) -> Option<String> {
    let lines = snapshot.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    for (start, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("define ") {
            continue;
        }
        let Some(name) = parse_define_symbol(trimmed) else {
            continue;
        };
        if !name.eq_ignore_ascii_case(selected_symbol) {
            continue;
        }

        let mut end = start;
        while end < lines.len() && lines[end].trim() != "}" {
            end += 1;
        }
        if end >= lines.len() {
            end = lines.len() - 1;
        }
        return Some(lines[start..=end].join("\n"));
    }

    None
}

fn parse_define_symbol(define_line: &str) -> Option<String> {
    let at_idx = define_line.find('@')?;
    let symbol_with_rest = &define_line[(at_idx + 1)..];
    if symbol_with_rest.is_empty() {
        return None;
    }

    if let Some(stripped) = symbol_with_rest.strip_prefix('"') {
        let end_quote = stripped.find('"')?;
        let symbol = &stripped[..end_quote];
        if symbol.is_empty() {
            return None;
        }
        return Some(symbol.to_string());
    }

    let end = symbol_with_rest.find('(').unwrap_or(symbol_with_rest.len());
    if end == 0 {
        return None;
    }
    let symbol = symbol_with_rest[..end].trim();
    if symbol.is_empty() {
        return None;
    }
    Some(symbol.to_string())
}

#[allow(dead_code)]
fn collect_step_remark_indices(pass: &str, target: &str, remarks: &[RemarkEntry]) -> Vec<usize> {
    let pass_norm = normalize_pass_name(pass);
    let target_is_module = target == "[module]";
    let matching = remarks
        .iter()
        .enumerate()
        .filter(|(_, remark)| normalize_pass_name(&remark.pass) == pass_norm)
        .collect::<Vec<_>>();

    if target_is_module {
        return matching.into_iter().map(|(idx, _)| idx).collect();
    }

    let exact_function = matching
        .iter()
        .filter_map(|(idx, remark)| {
            if remark
                .function
                .as_deref()
                .is_some_and(|func| func == target)
            {
                Some(*idx)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if !exact_function.is_empty() {
        return exact_function;
    }

    matching.into_iter().map(|(idx, _)| idx).collect()
}

#[allow(dead_code)]
fn normalize_pass_name(name: &str) -> String {
    normalize_pass_key(name)
}

fn normalize_pass_key(name: &str) -> String {
    let lowercase = name.to_ascii_lowercase();
    let without_suffix = lowercase.strip_suffix("pass").unwrap_or(&lowercase);
    without_suffix
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

fn truncate_lines(text: &str, max_lines: usize) -> String {
    let mut lines = text.lines();
    let kept = lines.by_ref().take(max_lines).collect::<Vec<_>>();
    let dropped = lines.count();
    if dropped == 0 {
        return text.to_string();
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("... [truncated {dropped} lines]"));
    out
}

fn parse_kind(header: &str) -> RemarkKind {
    if header.starts_with("--- !Passed") {
        RemarkKind::Passed
    } else if header.starts_with("--- !Missed") {
        RemarkKind::Missed
    } else if header.starts_with("--- !Analysis") {
        RemarkKind::Analysis
    } else {
        RemarkKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_tsvc_rows() {
        let text = r#"
Running each loop 100 times...

Loop 	 Time(Sec) 	 Checksum
S121	 0.00 		32007.271623919
S122	 1.25 		32164.490281733
"#;
        let rows = parse_tsvc_stdout(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].loop_id, "S121");
        assert!((rows[1].time_sec - 1.25).abs() < f64::EPSILON);
        assert_eq!(rows[1].checksum, "32164.490281733");
    }

    #[test]
    fn parses_all_remarks() {
        let sample = r#"
--- !Passed
Pass:            loop-vectorize
Name:            Vectorized
DebugLoc:        { File: 'foo.c', Line: 12, Column: 3 }
Function:        main
Args:
  - String:          'vectorized loop'
...
--- !Missed
Pass:            loop-vectorize
Name:            MissedDetails
DebugLoc:        { File: 'foo.c', Line: 22, Column: 3 }
Function:        main
Args:
  - String:          loop not vectorized
...
--- !Passed
Pass:            licm
Name:            Hoisted
...
"#;

        let mut path = std::env::temp_dir();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after unix epoch")
            .as_nanos();
        path.push(format!("tsvc-tui-remark-{unique}.opt.yaml"));
        fs::write(&path, sample).expect("sample remark file should be writable");

        let entries = parse_opt_remarks(&path).expect("remarks should parse");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "Vectorized");
        assert_eq!(entries[1].kind, RemarkKind::Missed);
        assert_eq!(entries[2].pass, "licm");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn groups_optimization_steps_by_pass_in_first_seen_order() {
        let entries = vec![
            RemarkEntry {
                kind: RemarkKind::Passed,
                pass: String::from("licm"),
                name: String::from("Hoisted"),
                file: None,
                line: None,
                function: None,
                message: None,
            },
            RemarkEntry {
                kind: RemarkKind::Missed,
                pass: String::from("loop-vectorize"),
                name: String::from("MissedDetails"),
                file: None,
                line: None,
                function: None,
                message: None,
            },
            RemarkEntry {
                kind: RemarkKind::Analysis,
                pass: String::from("licm"),
                name: String::from("LoadClobbered"),
                file: None,
                line: None,
                function: None,
                message: None,
            },
        ];

        let steps = group_optimization_steps(&entries);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].pass, "licm");
        assert_eq!(steps[0].total, 2);
        assert_eq!(steps[0].passed, 1);
        assert_eq!(steps[0].analysis, 1);
        assert_eq!(steps[0].remark_indices, vec![0, 2]);

        assert_eq!(steps[1].pass, "loop-vectorize");
        assert_eq!(steps[1].total, 1);
        assert_eq!(steps[1].missed, 1);
        assert_eq!(steps[1].remark_indices, vec![1]);
    }

    #[test]
    fn parses_ir_diff_steps_in_execution_order() {
        let trace = r#"
*** IR Dump At Start ***
define void @foo() {
entry:
  ret void
}
*** IR Dump After Annotation2MetadataPass on [module] omitted because no change ***
*** IR Dump After LICMPass on foo ***
define void @foo() {
entry:
  %x = add i32 1, 2
  ret void
}
*** IR Dump After LoopVectorizePass on foo ***
define void @foo() {
entry:
  %x = add i32 1, 2
  %v = insertelement <4 x i32> poison, i32 1, i64 0
  ret void
}
"#;
        let remarks = vec![
            RemarkEntry {
                kind: RemarkKind::Passed,
                pass: String::from("licm"),
                name: String::from("Hoisted"),
                file: None,
                line: None,
                function: Some(String::from("foo")),
                message: None,
            },
            RemarkEntry {
                kind: RemarkKind::Analysis,
                pass: String::from("loop-vectorize"),
                name: String::from("InterleavingNotBeneficial"),
                file: None,
                line: None,
                function: Some(String::from("foo")),
                message: None,
            },
        ];

        let steps = parse_ir_diff_steps(trace, &remarks);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].index, 1);
        assert_eq!(steps[0].pass, "LICMPass");
        assert_eq!(steps[0].target, "foo");
        assert!(steps[0].changed_lines > 0);
        assert_eq!(steps[0].remark_indices, vec![0]);

        assert_eq!(steps[1].index, 2);
        assert_eq!(steps[1].pass, "LoopVectorizePass");
        assert_eq!(steps[1].remark_indices, vec![1]);
        assert!(steps[1].diff_text.contains("@@"));
    }

    #[test]
    fn module_target_collects_all_matching_pass_remarks() {
        let trace = r#"
*** IR Dump At Start ***
; start
*** IR Dump After SimplifyCFGPass on [module] ***
; changed
"#;
        let remarks = vec![
            RemarkEntry {
                kind: RemarkKind::Analysis,
                pass: String::from("simplifycfg"),
                name: String::from("X"),
                file: None,
                line: None,
                function: Some(String::from("foo")),
                message: None,
            },
            RemarkEntry {
                kind: RemarkKind::Passed,
                pass: String::from("simplify-cfg"),
                name: String::from("Y"),
                file: None,
                line: None,
                function: Some(String::from("bar")),
                message: None,
            },
        ];

        let steps = parse_ir_diff_steps(trace, &remarks);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].remark_indices, vec![0, 1]);
    }

    #[test]
    fn fast_analysis_builds_changed_only_function_timeline() {
        let trace = r#"
*** IR Dump At Start ***
define void @foo() {
entry:
  ret void
}
*** IR Dump After SROAPass on foo omitted because no change ***
*** IR Dump After LICMPass on foo ***
define void @foo() {
entry:
  %x = add i32 1, 2
  ret void
}
*** IR Dump After GlobalOptPass on [module] ***
define void @foo() {
entry:
  %x = add i32 1, 2
  ret void
}
*** IR Dump After LoopVectorizePass on loop %L in function foo ***
define void @foo() {
entry:
  %x = add i32 1, 2
  %v = insertelement <4 x i32> poison, i32 1, i64 0
  ret void
}
"#;

        let remarks = vec![
            RemarkEntry {
                kind: RemarkKind::Passed,
                pass: String::from("licm"),
                name: String::from("Hoisted"),
                file: None,
                line: None,
                function: Some(String::from("foo")),
                message: None,
            },
            RemarkEntry {
                kind: RemarkKind::Analysis,
                pass: String::from("loop-vectorize"),
                name: String::from("InterleavingNotBeneficial"),
                file: None,
                line: None,
                function: Some(String::from("foo")),
                message: None,
            },
        ];

        let steps = build_fast_analysis_steps(trace, "foo", &remarks);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].visible_index, 0);
        assert_eq!(steps[0].raw_index, 0);
        assert_eq!(steps[1].raw_index, 2);
        assert_eq!(steps[1].pass, "LICMPass");
        assert_eq!(steps[1].pass_occurrence, 1);
        assert_eq!(steps[1].remark_indices, vec![0]);
        assert_eq!(steps[2].pass, "LoopVectorizePass");
        assert_eq!(steps[2].pass_occurrence, 1);
        assert_eq!(steps[2].remark_indices, vec![1]);
        assert!(steps[2].diff_text.contains("@@"));
    }

    #[test]
    fn deep_analysis_target_parsing_tracks_target_function() {
        let snapshots = vec![
            IrSnapshot {
                raw_index: 0,
                pass: String::from("(initial IR)"),
                pass_occurrence: 1,
                target: String::from("[module]"),
                snapshot: String::from("define void @foo() {\n  ret void\n}"),
            },
            IrSnapshot {
                raw_index: 10,
                pass: String::from("InlinerPass"),
                pass_occurrence: 1,
                target: String::from("(foo)"),
                snapshot: String::from("define void @foo() {\n  %x = add i32 1, 2\n  ret void\n}"),
            },
        ];
        let steps = build_analysis_steps_from_snapshots(
            &snapshots,
            "foo",
            &[],
            AnalysisSource::TraceFast,
        );
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].target_function.as_deref(), Some("foo"));
        assert_eq!(steps[1].source, AnalysisSource::TraceFast);
    }

    #[test]
    fn parses_dbg_locations_from_module_ir() {
        let ir = r#"
define void @foo() !dbg !5 {
  %1 = load float, ptr %p, !dbg !10
  ret void
}

!5 = distinct !DISubprogram(name: "foo")
!10 = !DILocation(line: 42, column: 5, scope: !5)
!11 = !DILocation(line: 43, column: 3, scope: !5)
"#;
        let locs = parse_dbg_locations(ir);
        assert_eq!(locs.get(&10), Some(&42));
        assert_eq!(locs.get(&11), Some(&43));
        assert_eq!(locs.get(&5), None);
    }

    #[test]
    fn builds_source_line_map_from_ir_lines() {
        let ir_lines = vec![
            IrLine {
                tag: ChangeTag::Equal,
                text: String::from("define void @foo() {"),
            },
            IrLine {
                tag: ChangeTag::Delete,
                text: String::from("  %1 = load float, ptr %p, !dbg !10"),
            },
            IrLine {
                tag: ChangeTag::Insert,
                text: String::from("  %wide = load <4 x float>, ptr %p, !dbg !10"),
            },
            IrLine {
                tag: ChangeTag::Equal,
                text: String::from("  ret void"),
            },
        ];
        let mut dbg_locations = HashMap::new();
        dbg_locations.insert(10, 42);

        let map = build_source_line_map(&ir_lines, &dbg_locations);
        assert_eq!(map.len(), 4);
        assert_eq!(map[0], None);
        assert_eq!(map[1], Some(42));
        assert_eq!(map[2], Some(42));
        assert_eq!(map[3], None);
    }

    #[test]
    fn source_line_map_graceful_when_no_dbg() {
        let ir_lines = vec![
            IrLine {
                tag: ChangeTag::Equal,
                text: String::from("  %x = add i32 1, 2"),
            },
        ];
        let dbg_locations = HashMap::new();
        let map = build_source_line_map(&ir_lines, &dbg_locations);
        assert_eq!(map, vec![None]);
    }
}
