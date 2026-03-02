use std::fs;
use std::path::Path;

use regex::Regex;

use crate::error::AppResult;
use crate::model::{LoopResult, RemarkEntry, RemarkKind};

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

        let pass_value = pass.unwrap_or_default();
        if pass_value != "loop-vectorize" {
            continue;
        }

        entries.push(RemarkEntry {
            kind,
            pass: pass_value,
            name: name.unwrap_or_default(),
            file,
            line,
            function,
            message,
        });
    }

    Ok(entries)
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
    fn parses_loop_vectorize_remarks() {
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
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "Vectorized");
        assert_eq!(entries[1].kind, RemarkKind::Missed);

        let _ = fs::remove_file(path);
    }
}
