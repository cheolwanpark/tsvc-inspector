use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, anyhow};
use regex::Regex;

use crate::benchmark_manifest::BENCHMARK_MANIFEST;
use crate::error::AppResult;
use crate::model::{BenchmarkFunction, BenchmarkItem};

pub fn discover_benchmarks(tsvc_root: &Path) -> AppResult<Vec<BenchmarkItem>> {
    let tsvc_dir = tsvc_root
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC");
    if !tsvc_dir.is_dir() {
        return Err(anyhow!("TSVC directory not found: {}", tsvc_dir.display()));
    }

    let mut benchmarks = Vec::new();
    for manifest in BENCHMARK_MANIFEST {
        let benchmark_dir = tsvc_dir.join(manifest.name);
        if !benchmark_dir.is_dir() {
            continue;
        }

        let (source_code, available_functions) = load_source_code_and_functions(&benchmark_dir);
        let (category, data_type) = split_category_type(manifest.name);
        benchmarks.push(BenchmarkItem {
            name: manifest.name.to_string(),
            category,
            data_type,
            run_options: manifest
                .run_options
                .iter()
                .map(ToString::to_string)
                .collect(),
            available_functions,
            source_code,
        });
    }

    benchmarks.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then(a.data_type.cmp(&b.data_type))
            .then(a.name.cmp(&b.name))
    });
    Ok(benchmarks)
}

fn load_source_code_and_functions(benchmark_dir: &Path) -> (String, Vec<BenchmarkFunction>) {
    let source_path = benchmark_dir.join("tsc.c");
    let source = match fs::read_to_string(&source_path) {
        Ok(source) => source,
        Err(err) => {
            let message = format!(
                "Source unavailable: failed to read {} ({err})",
                source_path.display()
            );
            return (message, Vec::new());
        }
    };

    let Some(tests_expr) = extract_tests_expression(&source) else {
        return (expand_tabs(&source), Vec::new());
    };
    let tests_flags = extract_test_flags(&tests_expr);
    if tests_flags.is_empty() {
        return (expand_tabs(&source), Vec::new());
    }

    let tsc_inc_path = benchmark_dir.join("..").join("tsc.inc");
    let available_functions =
        extract_available_functions(&tsc_inc_path, &tests_flags).unwrap_or_else(|_| Vec::new());
    let sections = match extract_relevant_tsc_sections(&tsc_inc_path, &tests_flags) {
        Ok(sections) if !sections.is_empty() => sections,
        _ => return (expand_tabs(&source), available_functions),
    };

    let mut filtered = String::from("/* Filtered TSVC benchmark source (kernel-focused) */\n");
    for line in source.lines() {
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
    tsc_inc_path: &Path,
    tests_flags: &HashSet<String>,
) -> AppResult<Vec<String>> {
    let source = fs::read_to_string(tsc_inc_path)
        .with_context(|| format!("read {}", tsc_inc_path.display()))?;
    let lines = source.lines().collect::<Vec<_>>();
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
    tsc_inc_path: &Path,
    tests_flags: &HashSet<String>,
) -> AppResult<Vec<BenchmarkFunction>> {
    let source = fs::read_to_string(tsc_inc_path)
        .with_context(|| format!("read {}", tsc_inc_path.display()))?;
    let call_re = Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\s*\((.*)\);\s*$")?;
    let wrapped_call_re = Regex::new(
        r#"^if\s*\(\s*tsvc_tui_should_run\("([A-Za-z_][A-Za-z0-9_]*)"\)\s*\)\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(.*\);\s*$"#,
    )?;

    let mut in_main = false;
    let mut in_call_block = false;
    let mut conditions = Vec::<bool>::new();
    let mut functions = Vec::new();
    let mut seen = HashSet::new();

    for line in source.lines() {
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

fn expand_tabs(text: &str) -> String {
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

fn split_category_type(dir_name: &str) -> (String, String) {
    if let Some((category, data_type)) = dir_name.rsplit_once('-') {
        (category.to_string(), data_type.to_string())
    } else {
        (dir_name.to_string(), "unknown".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark_manifest;

    #[test]
    fn manifest_contains_known_target() {
        let entry = benchmark_manifest::find("InductionVariable-dbl")
            .expect("manifest must include InductionVariable-dbl");
        assert_eq!(entry.run_options, &["9100", "14"]);
    }

    #[test]
    fn splits_unknown_type() {
        let (category, data_type) = split_category_type("SomeSuite");
        assert_eq!(category, "SomeSuite");
        assert_eq!(data_type, "unknown");
    }

    #[test]
    fn load_source_code_returns_fallback_message_on_missing_file() {
        let missing_dir = Path::new("/definitely-missing-tsvc-benchmark-dir");
        let (source, functions) = load_source_code_and_functions(missing_dir);
        assert!(source.starts_with("Source unavailable: failed to read "));
        assert!(source.contains("tsc.c"));
        assert!(functions.is_empty());
    }

    #[test]
    fn load_source_code_reads_tsc_c_when_present() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tsvc-tui-source-test-{unique}"));
        fs::create_dir_all(&dir).expect("create temp benchmark dir");
        fs::write(dir.join("tsc.c"), "int tsvc_test(void) {\n  return 7;\n}\n")
            .expect("write tsc.c");

        let (source, functions) = load_source_code_and_functions(&dir);
        assert!(source.contains("tsvc_test"));
        assert!(functions.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn extracts_tests_expression_from_tsc_c_define() {
        let source = "#define TYPE double\n#define TESTS CONTROL_FLOW\n#include \"../tsc.inc\"\n";
        assert_eq!(
            extract_tests_expression(source).as_deref(),
            Some("CONTROL_FLOW")
        );
    }

    #[test]
    fn load_source_code_keeps_selected_kernel_sections_only() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tsvc-tui-filter-test-{unique}"));
        let benchmark_dir = root.join("Demo-dbl");
        fs::create_dir_all(&benchmark_dir).expect("create benchmark dir");
        fs::write(
            benchmark_dir.join("tsc.c"),
            "#define TYPE double\n#define ALIGNMENT 32\n#define TESTS CONTROL_FLOW\n#include \"../tsc.inc\"\n",
        )
        .expect("write tsc.c");
        fs::write(
            root.join("tsc.inc"),
            r#"
#if TESTS & CONTROL_FLOW
int keep_me() {
    clock_t start_t, end_t, clock_dif; double clock_dif_sec;
    init("keep");
    for (int i = 0; i < LEN; i++) {
        a[i] += b[i] * c[i];
    }
    dummy(a, b, c, d, e, aa, bb, cc, 0.);
    printf("SKEEP\t %.2f \t\t", clock_dif_sec);
    check(1);
    return 0;
}
#endif // TESTS & CONTROL_FLOW

#if TESTS & SYMBOLICS
int drop_me() { return 0; }
#endif // TESTS & SYMBOLICS
"#,
        )
        .expect("write tsc.inc");

        let (source, functions) = load_source_code_and_functions(&benchmark_dir);
        assert!(source.contains("#define TESTS CONTROL_FLOW"));
        assert!(source.contains("int keep_me()"));
        assert!(source.contains("for (int i = 0; i < LEN; i++)"));
        assert!(!source.contains("int drop_me()"));
        assert!(!source.contains("clock_t start_t"));
        assert!(!source.contains("dummy("));
        assert!(!source.contains("printf(\"SKEEP"));
        assert!(functions.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn expands_tabs_for_visible_indentation() {
        let input = "\tif (x) {\n\t\treturn 1;\n\t}\n";
        let expanded = expand_tabs(input);
        assert!(expanded.contains("    if (x) {"));
        assert!(expanded.contains("        return 1;"));
        assert!(!expanded.contains('\t'));
    }

    #[test]
    fn extracts_available_functions_from_main_call_list() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tsvc-tui-function-test-{unique}"));
        fs::create_dir_all(&root).expect("create temp root");
        let tsc_inc = root.join("tsc.inc");
        fs::write(
            &tsc_inc,
            r#"
int main(int argc, char **argv) {
    printf("Loop \t Time(Sec) \t Checksum \n");
#if TESTS & CONTROL_FLOW
    s161();
    s162(1);
#endif
#if TESTS & CONTROL_LOOPS
    va();
#endif
    return 0;
}
"#,
        )
        .expect("write tsc.inc");
        let tests_flags = HashSet::from([String::from("CONTROL_FLOW")]);

        let functions =
            extract_available_functions(&tsc_inc, &tests_flags).expect("function extraction");
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0].loop_id, "S161");
        assert_eq!(functions[0].symbol, "s161");
        assert_eq!(functions[1].loop_id, "S162");
        assert_eq!(functions[1].symbol, "s162");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extracts_functions_from_patched_wrapper_lines() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tsvc-tui-function-wrapped-test-{unique}"));
        fs::create_dir_all(&root).expect("create temp root");
        let tsc_inc = root.join("tsc.inc");
        fs::write(
            &tsc_inc,
            r#"
int main(int argc, char **argv) {
    printf("Loop \t Time(Sec) \t Checksum \n");
#if TESTS & CONTROL_FLOW
    if (tsvc_tui_should_run("s161")) s161();
    if (tsvc_tui_should_run("s162")) s162(n1);
#endif
    return 0;
}
"#,
        )
        .expect("write tsc.inc");
        let tests_flags = HashSet::from([String::from("CONTROL_FLOW")]);

        let functions =
            extract_available_functions(&tsc_inc, &tests_flags).expect("function extraction");
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0].loop_id, "S161");
        assert_eq!(functions[0].symbol, "s161");
        assert_eq!(functions[1].loop_id, "S162");
        assert_eq!(functions[1].symbol, "s162");

        let _ = fs::remove_dir_all(root);
    }
}
