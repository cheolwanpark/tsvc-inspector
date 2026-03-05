use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use similar::{ChangeTag, TextDiff};

use crate::core::model::{
    AnalysisSource, AnalysisStage, AnalysisStep, DbgLocation, IrLine, RemarkEntry,
};
use crate::data::parser::{normalize_pass_key, parse_dbg_locations, parse_ir_snapshots_from_trace};

const MAX_DIFF_LINES: usize = 8000;

pub fn build_fast_analysis_steps(
    build_trace: &str,
    selected_function_symbol: &str,
    remarks: &[RemarkEntry],
    source_file_content: Option<&str>,
) -> Vec<AnalysisStep> {
    let snapshots = parse_ir_snapshots_from_trace(build_trace);
    build_analysis_steps_from_snapshots(
        &snapshots,
        selected_function_symbol,
        remarks,
        AnalysisSource::TraceFast,
        source_file_content,
    )
}

pub fn annotate_ir_lines(
    ir_lines: Vec<IrLine>,
    dbg_after: &HashMap<u32, DbgLocation>,
    dbg_before: Option<&HashMap<u32, DbgLocation>>,
    source_lines: &[&str],
) -> (Vec<IrLine>, Vec<Option<u32>>) {
    static DBG_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"!dbg !(\d+)").expect("valid dbg ref regex"));
    static META_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:,\s*|\s+)![\w.]+\s+!\d+").expect("valid trailing metadata regex")
    });

    let mut out_lines = Vec::with_capacity(ir_lines.len());
    let mut out_map = Vec::with_capacity(ir_lines.len());
    let mut prev_src_key: Option<(u32, Option<String>, ChangeTag)> = None;

    for ir_line in ir_lines {
        if ir_line.text.trim_start().starts_with("#dbg_") {
            continue;
        }

        let dbg_map = if ir_line.tag == ChangeTag::Delete {
            dbg_before.unwrap_or(dbg_after)
        } else {
            dbg_after
        };

        let dbg_loc = DBG_RE.captures(&ir_line.text).and_then(|caps| {
            let id: u32 = caps.get(1)?.as_str().parse().ok()?;
            dbg_map.get(&id)
        });
        let src_line_no = dbg_loc.map(|loc| loc.line);

        if let Some(loc) = dbg_loc {
            let current_key = (loc.line, loc.inlined_from.clone(), ir_line.tag);
            let changed = prev_src_key.as_ref() != Some(&current_key);
            if changed {
                if let Some(src) = source_lines
                    .get((loc.line as usize).checked_sub(1).unwrap_or(usize::MAX))
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    let annotation = match &loc.inlined_from {
                        Some(name) => format!(";; [{name}] {src}"),
                        None => format!(";; {src}"),
                    };
                    out_lines.push(IrLine {
                        tag: ir_line.tag,
                        text: annotation,
                        is_source_annotation: true,
                    });
                    out_map.push(Some(loc.line));
                }
                prev_src_key = Some(current_key);
            }
        }

        let stripped = META_RE.replace_all(&ir_line.text, "");
        let stripped = stripped.trim_end();

        out_map.push(src_line_no);
        out_lines.push(IrLine {
            tag: ir_line.tag,
            text: stripped.to_string(),
            is_source_annotation: false,
        });
    }

    (out_lines, out_map)
}

fn build_analysis_steps_from_snapshots(
    snapshots: &[crate::data::parser::IrSnapshot],
    selected_function_symbol: &str,
    remarks: &[RemarkEntry],
    source: AnalysisSource,
    source_file_content: Option<&str>,
) -> Vec<AnalysisStep> {
    let src_lines: Vec<&str> = source_file_content
        .map(|s| s.lines().collect())
        .unwrap_or_default();

    let mut out = Vec::new();
    let mut prev_ir: Option<String> = None;
    let mut prev_dbg_locations: Option<HashMap<u32, DbgLocation>> = None;
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
                    is_source_annotation: false,
                })
                .collect();
            let (ir_lines, source_line_map) =
                annotate_ir_lines(ir_lines, &dbg_locations, None, &src_lines);
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
            prev_dbg_locations = Some(dbg_locations);
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
                is_source_annotation: false,
            })
            .collect();
        let (ir_lines, source_line_map) = annotate_ir_lines(
            ir_lines,
            &dbg_locations,
            prev_dbg_locations.as_ref(),
            &src_lines,
        );

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
        prev_dbg_locations = Some(dbg_locations);
        prev_ir = Some(function_ir);
    }

    out
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

    let mut name = String::new();
    for ch in symbol_with_rest.chars() {
        if ch == '(' {
            break;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            name.push(ch);
        } else if name.is_empty() {
            continue;
        } else {
            break;
        }
    }

    if name.is_empty() { None } else { Some(name) }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::RemarkKind;

    #[test]
    fn fast_analysis_builds_changed_only_function_timeline() {
        let trace = r#"
*** IR Dump At Start ***
define i32 @s161() !dbg !14 {
entry:
  %0 = add i32 1, 2, !dbg !9
  ret i32 %0, !dbg !10
}
!9 = !DILocation(line: 10, column: 3, scope: !14)
!10 = !DILocation(line: 11, column: 3, scope: !14)
!14 = distinct !DISubprogram(name: "s161", scope: !1, file: !1, line: 1)
*** IR Dump After LoopVectorizePass on s161 ***
define i32 @s161() !dbg !14 {
entry:
  %0 = add i32 1, 4, !dbg !9
  ret i32 %0, !dbg !10
}
!9 = !DILocation(line: 10, column: 3, scope: !14)
!10 = !DILocation(line: 11, column: 3, scope: !14)
!14 = distinct !DISubprogram(name: "s161", scope: !1, file: !1, line: 1)
"#;

        let remarks = vec![RemarkEntry {
            kind: RemarkKind::Passed,
            pass: "loop-vectorize".to_string(),
            name: "Vectorized".to_string(),
            file: None,
            line: None,
            function: Some("s161".to_string()),
            message: Some("vectorized loop".to_string()),
        }];

        let steps = build_fast_analysis_steps(trace, "s161", &remarks, Some("line10\nline11\n"));
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].stage, AnalysisStage::Initial);
        assert_eq!(steps[1].stage, AnalysisStage::Vectorize);
        assert!(steps[1].changed_lines > 0);
    }
}
