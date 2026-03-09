use std::collections::HashMap;
use std::fs;
use std::path::Path;

use regex::Regex;

use crate::core::error::AppResult;
use crate::core::model::{DbgLocation, RemarkEntry, RemarkKind};

#[derive(Clone, Debug)]
pub struct IrSnapshot {
    pub raw_index: usize,
    pub pass: String,
    pub pass_occurrence: usize,
    pub target: String,
    pub snapshot: String,
}

#[derive(Clone, Debug)]
pub struct TracePassRecord {
    pub raw_index: usize,
    pub pass: String,
    pub pass_occurrence: usize,
    pub target: String,
    pub changed: bool,
    pub log_line: String,
}

#[derive(Clone, Debug)]
pub struct BisectPassRecord {
    pub order_index: usize,
    pub pass: String,
    pub pass_occurrence: usize,
    pub target: String,
    pub log_line: String,
}

pub fn parse_opt_remarks(path: &Path) -> AppResult<Vec<RemarkEntry>> {
    let content = fs::read_to_string(path)?;
    parse_opt_remarks_from_str(&content)
}

pub fn parse_opt_remarks_from_str(content: &str) -> AppResult<Vec<RemarkEntry>> {
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
        let mut args_started = false;
        let mut arg_parts = Vec::new();

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
            if l == "Args:" {
                args_started = true;
                continue;
            }
            if args_started && let Some(value) = parse_yaml_scalar_value(l) {
                arg_parts.push(value);
            }
        }
        if !arg_parts.is_empty() {
            message = Some(arg_parts.join(""));
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

pub fn parse_trace_pass_records(build_trace: &str) -> Vec<TracePassRecord> {
    let after_re = Regex::new(r"^\*\*\* IR Dump After (.+?) on (.+?) \*\*\*$")
        .expect("valid ir dump after regex");
    let no_change_re = Regex::new(r"^\*\*\* IR Dump After .+ omitted because no change \*\*\*$")
        .expect("valid ir no-change regex");

    let mut raw_after_index = 0usize;
    let mut pass_occurrence_by_key = HashMap::<String, usize>::new();
    let mut records = Vec::new();

    for line in build_trace.lines() {
        let header = line.trim();
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
        records.push(TracePassRecord {
            raw_index: raw_after_index,
            pass,
            pass_occurrence,
            target,
            changed: !no_change_re.is_match(header),
            log_line: header.to_string(),
        });
    }

    records
}

pub fn parse_bisect_pass_records(build_trace: &str) -> Vec<BisectPassRecord> {
    let bisect_re = Regex::new(r"^BISECT:\s+running pass \((\d+)\)\s+(.+?)\s+on\s+(.+?)\s*$")
        .expect("valid bisect running regex");
    let mut pass_occurrence_by_key = HashMap::<String, usize>::new();
    let mut records = Vec::new();

    for line in build_trace.lines() {
        let trimmed = line.trim();
        let Some(caps) = bisect_re.captures(trimmed) else {
            continue;
        };
        let order_index = caps
            .get(1)
            .and_then(|m| m.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        let pass = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| String::from("(unknown-pass)"));
        let target = caps
            .get(3)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| String::from("(unknown-target)"));
        let pass_key = normalize_pass_key(&pass);
        let pass_occurrence = {
            let next = pass_occurrence_by_key.get(&pass_key).copied().unwrap_or(0) + 1;
            pass_occurrence_by_key.insert(pass_key, next);
            next
        };
        records.push(BisectPassRecord {
            order_index,
            pass,
            pass_occurrence,
            target,
            log_line: trimmed.to_string(),
        });
    }

    records
}

pub fn parse_dbg_locations(module_ir: &str) -> HashMap<u32, DbgLocation> {
    let re =
        Regex::new(r"!(\d+) = !DILocation\(line: (\d+),.*?scope: !(\d+)(?:.*?(inlinedAt))?.*?\)")
            .expect("valid DILocation regex");

    let scope_names = resolve_scope_names(module_ir);
    let mut map = HashMap::new();

    for caps in re.captures_iter(module_ir) {
        if let (Some(id_match), Some(line_match), Some(scope_match)) =
            (caps.get(1), caps.get(2), caps.get(3))
            && let (Ok(id), Ok(line), Ok(scope_id)) = (
                id_match.as_str().parse::<u32>(),
                line_match.as_str().parse::<u32>(),
                scope_match.as_str().parse::<u32>(),
            )
        {
            let inlined_from = if caps.get(4).is_some() {
                scope_names.get(&scope_id).cloned()
            } else {
                None
            };
            map.insert(id, DbgLocation { line, inlined_from });
        }
    }
    map
}

pub fn normalize_pass_key(name: &str) -> String {
    let lowercase = name.to_ascii_lowercase();
    let without_suffix = lowercase.strip_suffix("pass").unwrap_or(&lowercase);
    without_suffix
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

fn parse_yaml_scalar_value(line: &str) -> Option<String> {
    let content = line
        .trim_start()
        .strip_prefix("- ")
        .unwrap_or(line.trim_start());
    let (_, raw_value) = content.split_once(':')?;
    let value = raw_value.trim();
    if value.is_empty() {
        return None;
    }
    Some(decode_yaml_scalar(value))
}

fn decode_yaml_scalar(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0] as char;
        let last = trimmed.as_bytes()[trimmed.len() - 1] as char;
        if (first == '\'' && last == '\'') || (first == '"' && last == '"') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn resolve_scope_names(module_ir: &str) -> HashMap<u32, String> {
    let subprogram_re =
        Regex::new(r#"!(\d+)\s*=\s*distinct\s+!DISubprogram\(.*?name:\s*"([^"]+)""#)
            .expect("valid DISubprogram regex");
    let block_re = Regex::new(r"!(\d+)\s*=\s*distinct\s+!DILexicalBlock\(.*?scope:\s*!(\d+)")
        .expect("valid DILexicalBlock regex");

    let mut subprogram_names: HashMap<u32, String> = HashMap::new();
    let mut block_parents: HashMap<u32, u32> = HashMap::new();

    for caps in subprogram_re.captures_iter(module_ir) {
        if let (Some(id_m), Some(name_m)) = (caps.get(1), caps.get(2))
            && let Ok(id) = id_m.as_str().parse::<u32>()
        {
            subprogram_names.insert(id, name_m.as_str().to_string());
        }
    }

    for caps in block_re.captures_iter(module_ir) {
        if let (Some(id_m), Some(parent_m)) = (caps.get(1), caps.get(2))
            && let (Ok(id), Ok(parent)) = (
                id_m.as_str().parse::<u32>(),
                parent_m.as_str().parse::<u32>(),
            )
        {
            block_parents.insert(id, parent);
        }
    }

    let mut resolved: HashMap<u32, String> = HashMap::new();

    let all_ids: Vec<u32> = subprogram_names
        .keys()
        .chain(block_parents.keys())
        .copied()
        .collect();

    for id in all_ids {
        if resolved.contains_key(&id) {
            continue;
        }
        let mut current = id;
        let mut depth = 0;
        loop {
            if let Some(name) = subprogram_names.get(&current) {
                resolved.insert(id, name.clone());
                break;
            }
            if let Some(&parent) = block_parents.get(&current) {
                depth += 1;
                if depth > 20 {
                    break;
                }
                current = parent;
            } else {
                break;
            }
        }
    }

    resolved
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
    use crate::core::model::RemarkKind;

    #[test]
    fn parses_dbg_locations_from_module_ir() {
        let ir = r#"
!9 = !DILocation(line: 42, column: 7, scope: !12)
!10 = !DILocation(line: 52, column: 3, scope: !13, inlinedAt: !99)
!12 = distinct !DISubprogram(name: "outer", scope: !1, file: !1, line: 1)
!13 = distinct !DISubprogram(name: "inner", scope: !1, file: !1, line: 2)
"#;
        let locs = parse_dbg_locations(ir);
        assert_eq!(locs.get(&9).map(|v| v.line), Some(42));
        assert_eq!(
            locs.get(&10).and_then(|v| v.inlined_from.as_deref()),
            Some("inner")
        );
    }

    #[test]
    fn parses_full_message_from_remark_args() {
        let yaml = r#"--- !Passed
Pass:            loop-vectorize
Name:            Vectorized
Function:        foo
Args:
  - String:          'vectorized '
  - String:          'loop (vectorization width: '
  - VectorizationFactor: '4'
  - String:          ', interleaved count: '
  - InterleaveCount: '4'
  - String:          ')'
..."#;

        let remarks = parse_opt_remarks_from_str(yaml).expect("yaml should parse");
        assert_eq!(remarks.len(), 1);
        assert_eq!(remarks[0].kind, RemarkKind::Passed);
        assert_eq!(
            remarks[0].message.as_deref(),
            Some("vectorized loop (vectorization width: 4, interleaved count: 4)")
        );
    }

    #[test]
    fn parses_trace_records_for_changed_and_no_change_passes() {
        let trace = r#"
*** IR Dump After LICMPass on loop %L1 in function foo ***
define void @foo() { ret void }
*** IR Dump After SROAPass on foo omitted because no change ***
"#;

        let records = parse_trace_pass_records(trace);
        assert_eq!(records.len(), 2);
        assert!(records[0].changed);
        assert!(!records[1].changed);
        assert_eq!(records[0].pass_occurrence, 1);
        assert_eq!(records[1].pass_occurrence, 1);
    }

    #[test]
    fn parses_bisect_running_pass_records() {
        let trace = r#"
BISECT: running pass (31) loop-vectorize on foo
BISECT: running pass (32) loop-vectorize on foo
"#;

        let records = parse_bisect_pass_records(trace);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].order_index, 31);
        assert_eq!(records[0].pass_occurrence, 1);
        assert_eq!(records[1].pass_occurrence, 2);
    }
}
