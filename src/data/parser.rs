use std::collections::HashMap;
use std::fs;
use std::path::Path;

use regex::Regex;

use crate::core::error::AppResult;
use crate::core::model::{DbgLocation, LoopResult, RemarkEntry, RemarkKind};

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

    #[test]
    fn parses_tsvc_rows() {
        let text = r#"
Running each loop 100 times...

Loop    Time(Sec)       Checksum
S121    0.00            32007.271623919
S122    1.25            32164.490281733
"#;
        let rows = parse_tsvc_stdout(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].loop_id, "S121");
        assert!((rows[1].time_sec - 1.25).abs() < f64::EPSILON);
        assert_eq!(rows[1].checksum, "32164.490281733");
    }

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
}
