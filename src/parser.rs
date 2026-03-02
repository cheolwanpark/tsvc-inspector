use std::collections::HashMap;
use std::fs;
use std::path::Path;

use regex::Regex;
use similar::{ChangeTag, TextDiff};

use crate::error::AppResult;
use crate::model::{IrDiffStep, LoopResult, OptimizationStep, RemarkEntry, RemarkKind};

const MAX_IR_STEPS: usize = 500;
const MAX_DIFF_LINES: usize = 8000;

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

fn normalize_pass_name(name: &str) -> String {
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
}
