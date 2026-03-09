use std::collections::HashSet;

use anyhow::{Context, Result};
use regex::Regex;

use crate::core::model::BenchmarkFunction;

pub fn build_kernel_focused_source_and_functions(
    tsc_source: &str,
    tsc_inc_source: Option<&str>,
) -> (String, Vec<BenchmarkFunction>) {
    let Some(tests_expr) = extract_tests_expression(tsc_source) else {
        return (expand_tabs(tsc_source), Vec::new());
    };
    let tests_flags = extract_test_flags(&tests_expr);
    if tests_flags.is_empty() {
        return (expand_tabs(tsc_source), Vec::new());
    }

    let Some(tsc_inc_source) = tsc_inc_source else {
        return (expand_tabs(tsc_source), Vec::new());
    };

    let available_functions =
        extract_available_functions(tsc_inc_source, &tests_flags).unwrap_or_else(|_| Vec::new());
    let sections = match extract_relevant_tsc_sections(tsc_inc_source, &tests_flags) {
        Ok(sections) if !sections.is_empty() => sections,
        _ => return (expand_tabs(tsc_source), available_functions),
    };

    let mut filtered = String::from("/* Filtered TSVC benchmark source (kernel-focused) */\n");
    for line in tsc_source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#define TYPE")
            || trimmed.starts_with("#define ALIGNMENT")
            || trimmed.starts_with("#define TESTS")
        {
            filtered.push_str(trimmed);
            filtered.push('\n');
        }
    }
    filtered.push('\n');

    for section in sections {
        filtered.push_str(&section);
        if !section.ends_with('\n') {
            filtered.push('\n');
        }
        filtered.push('\n');
    }

    (expand_tabs(&filtered), available_functions)
}

pub fn extract_c_function_source(source: &str, symbol: &str) -> Option<String> {
    if symbol.is_empty() {
        return None;
    }
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return None;
    }

    for start in 0..lines.len() {
        if !line_contains_symbol_with_call_paren(lines[start], symbol) {
            continue;
        }

        let mut found_open_brace = false;
        let mut brace_depth = 0u32;
        let mut end = start;

        while end < lines.len() {
            let line = lines[end];
            for ch in line.chars() {
                if found_open_brace {
                    match ch {
                        '{' => brace_depth = brace_depth.saturating_add(1),
                        '}' => {
                            brace_depth = brace_depth.saturating_sub(1);
                            if brace_depth == 0 {
                                return Some(lines[start..=end].join("\n"));
                            }
                        }
                        _ => {}
                    }
                } else {
                    match ch {
                        ';' => break,
                        '{' => {
                            found_open_brace = true;
                            brace_depth = 1;
                        }
                        _ => {}
                    }
                }
            }

            if !found_open_brace && line.contains(';') {
                break;
            }
            end += 1;
        }
    }

    None
}

fn extract_tests_expression(source: &str) -> Option<String> {
    for line in source.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("#define") {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        if parts.next() != Some("#define") || parts.next() != Some("TESTS") {
            continue;
        }
        let expr = parts.collect::<Vec<_>>().join(" ");
        let expr = expr
            .split("//")
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        if !expr.is_empty() {
            return Some(expr);
        }
    }
    None
}

fn extract_test_flags(expr: &str) -> HashSet<String> {
    expr.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|token| !token.is_empty())
        .filter(|token| *token != "TESTS")
        .filter(|token| token.chars().any(|c| c.is_ascii_alphabetic()))
        .map(ToString::to_string)
        .collect()
}

fn extract_relevant_tsc_sections(
    tsc_inc_source: &str,
    tests_flags: &HashSet<String>,
) -> Result<Vec<String>> {
    let lines = tsc_inc_source.lines().collect::<Vec<_>>();
    let mut sections = Vec::new();
    let mut idx = 0usize;

    while idx < lines.len() {
        if !is_relevant_tests_guard(lines[idx], tests_flags) {
            idx += 1;
            continue;
        }

        let start = idx;
        idx += 1;
        let mut depth = 1usize;
        while idx < lines.len() && depth > 0 {
            let trimmed = lines[idx].trim_start();
            if trimmed.starts_with("#if") {
                depth += 1;
            } else if trimmed.starts_with("#endif") {
                depth = depth.saturating_sub(1);
            }
            idx += 1;
        }

        let section = lines[start..idx.min(lines.len())].join("\n");
        let filtered_section = filter_section_noise(&section);
        if !filtered_section.trim().is_empty() {
            sections.push(filtered_section);
        }
    }

    Ok(sections)
}

fn extract_available_functions(
    tsc_inc_source: &str,
    tests_flags: &HashSet<String>,
) -> Result<Vec<BenchmarkFunction>> {
    let call_re =
        Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\s*\((.*)\);\s*$").context("build call regex")?;
    let wrapped_call_re = Regex::new(
        r#"^if\s*\(\s*tsvc_inspector_should_run\("([A-Za-z_][A-Za-z0-9_]*)"\)\s*\)\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(.*\);\s*$"#,
    )
    .context("build wrapped call regex")?;

    let mut in_main = false;
    let mut in_call_block = false;
    let mut conditions = Vec::<bool>::new();
    let mut functions = Vec::new();
    let mut seen = HashSet::new();

    for line in tsc_inc_source.lines() {
        let trimmed = line.trim_start();
        if !in_main {
            if trimmed.starts_with("int main(") {
                in_main = true;
            }
            continue;
        }

        if !in_call_block && trimmed.contains("printf(\"Loop") {
            in_call_block = true;
            continue;
        }
        if !in_call_block {
            continue;
        }
        if trimmed.starts_with("return ") {
            break;
        }

        if trimmed.starts_with("#if") {
            let relevant = if trimmed.contains("TESTS") {
                let line_flags = extract_test_flags(trimmed);
                tests_flags.iter().any(|flag| line_flags.contains(flag))
            } else {
                true
            };
            conditions.push(relevant);
            continue;
        }
        if trimmed.starts_with("#endif") {
            conditions.pop();
            continue;
        }
        if !conditions.iter().all(|ok| *ok) {
            continue;
        }

        let symbol = if let Some(caps) = wrapped_call_re.captures(trimmed) {
            let filter_symbol = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let call_symbol = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
            if !filter_symbol.is_empty() && filter_symbol == call_symbol {
                filter_symbol.to_string()
            } else if !call_symbol.is_empty() {
                call_symbol.to_string()
            } else {
                continue;
            }
        } else {
            let Some(caps) = call_re.captures(trimmed) else {
                continue;
            };
            let Some(symbol_match) = caps.get(1) else {
                continue;
            };
            let symbol = symbol_match.as_str();
            if is_control_keyword(symbol) {
                continue;
            }
            symbol.to_string()
        };

        if !seen.insert(symbol.clone()) {
            continue;
        }
        functions.push(BenchmarkFunction {
            loop_id: symbol_to_loop_id(&symbol),
            symbol,
        });
    }

    Ok(functions)
}

fn is_control_keyword(symbol: &str) -> bool {
    matches!(symbol, "if" | "for" | "while" | "switch")
}

fn symbol_to_loop_id(symbol: &str) -> String {
    if symbol.starts_with('s') && symbol.chars().skip(1).all(|c| c.is_ascii_digit()) {
        format!("S{}", &symbol[1..])
    } else {
        symbol.to_string()
    }
}

fn is_relevant_tests_guard(line: &str, tests_flags: &HashSet<String>) -> bool {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("#if") || !trimmed.contains("TESTS") {
        return false;
    }
    let line_flags = extract_test_flags(trimmed);
    tests_flags.iter().any(|flag| line_flags.contains(flag))
}

fn filter_section_noise(section: &str) -> String {
    let mut lines = section.lines().collect::<Vec<_>>();
    if lines
        .first()
        .is_some_and(|line| line.trim_start().starts_with("#if"))
    {
        lines.remove(0);
    }
    if lines
        .last()
        .is_some_and(|line| line.trim_start().starts_with("#endif"))
    {
        lines.pop();
    }

    let mut out = Vec::new();
    let mut previous_was_blank = false;
    for line in lines {
        let trimmed = line.trim();
        if should_skip_section_line(trimmed) {
            continue;
        }
        if trimmed.is_empty() {
            if !previous_was_blank {
                out.push(String::new());
            }
            previous_was_blank = true;
            continue;
        }
        previous_was_blank = false;
        out.push(line.to_string());
    }

    while out.first().is_some_and(|line| line.trim().is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|line| line.trim().is_empty()) {
        out.pop();
    }

    out.join("\n")
}

fn should_skip_section_line(trimmed: &str) -> bool {
    trimmed.starts_with("clock_t start_t")
        || trimmed.starts_with("start_t = clock()")
        || trimmed.starts_with("end_t = clock()")
        || trimmed.starts_with("clock_dif_sec =")
        || trimmed.starts_with("printf(\"S")
        || trimmed.starts_with("check(")
        || trimmed.starts_with("dummy(")
        || trimmed.starts_with("init(")
        || trimmed.starts_with("// %")
}

pub fn expand_tabs(text: &str) -> String {
    let mut output = String::new();
    for line in text.lines() {
        output.push_str(&expand_tabs_in_line(line, 4));
        output.push('\n');
    }
    output
}

fn expand_tabs_in_line(line: &str, tab_width: usize) -> String {
    let mut output = String::new();
    let mut column = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let spaces = tab_width - (column % tab_width);
            output.push_str(&" ".repeat(spaces));
            column += spaces;
            continue;
        }
        output.push(ch);
        column += 1;
    }
    output
}

fn line_contains_symbol_with_call_paren(line: &str, symbol: &str) -> bool {
    let bytes = line.as_bytes();
    let symbol_len = symbol.len();
    if symbol_len == 0 || bytes.len() < symbol_len {
        return false;
    }

    let mut offset = 0usize;
    while offset + symbol_len <= bytes.len() {
        let Some(found) = line[offset..].find(symbol) else {
            break;
        };
        let start = offset + found;
        let end = start + symbol_len;

        let left_ok = start == 0 || !is_identifier_byte(bytes[start - 1]);
        let right_ok = end >= bytes.len() || !is_identifier_byte(bytes[end]);
        if left_ok && right_ok {
            let mut idx = end;
            while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
                idx += 1;
            }
            if idx < bytes.len() && bytes[idx] == b'(' {
                return true;
            }
        }
        offset = end;
    }
    false
}

fn is_identifier_byte(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_c_function_source_returns_only_target_function() {
        let source = r#"
int s160() {
    return 0;
}

int s161() {
    int acc = 0;
    acc += 1;
    return acc;
}

int s162() {
    return 2;
}
"#;

        let extracted =
            extract_c_function_source(source, "s161").expect("target function should be found");
        assert!(extracted.contains("int s161()"));
        assert!(extracted.contains("return acc;"));
        assert!(!extracted.contains("int s160()"));
        assert!(!extracted.contains("int s162()"));
    }

    #[test]
    fn expands_tabs_for_visible_indentation() {
        let input = "\tif (x) {\n\t\treturn 1;\n\t}\n";
        let expanded = expand_tabs(input);
        assert!(expanded.contains("    if (x) {"));
        assert!(expanded.contains("        return 1;"));
        assert!(!expanded.contains('\t'));
    }
}
