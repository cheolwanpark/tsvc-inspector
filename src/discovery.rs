use std::fs;
use std::path::Path;

use anyhow::{Context, anyhow};
use regex::Regex;

use crate::error::AppResult;
use crate::model::BenchmarkItem;

pub fn discover_benchmarks(tsvc_root: &Path) -> AppResult<Vec<BenchmarkItem>> {
    let tsvc_dir = tsvc_root
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC");
    if !tsvc_dir.is_dir() {
        return Err(anyhow!("TSVC directory not found: {}", tsvc_dir.display()));
    }

    let target_re = Regex::new(r"llvm_multisource\(\s*([^) \t\r\n]+)\s*\)")?;
    let run_opts_re = Regex::new(r"set\(\s*RUN_OPTIONS\s+([^)]+)\)")?;

    let mut benchmarks = Vec::new();
    for entry in fs::read_dir(&tsvc_dir).with_context(|| format!("read {}", tsvc_dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let dir_name = entry.file_name().to_string_lossy().to_string();
        let cmake_path = path.join("CMakeLists.txt");
        if !cmake_path.exists() {
            continue;
        }

        let content = fs::read_to_string(&cmake_path)
            .with_context(|| format!("read {}", cmake_path.display()))?;
        if let Some(item) = parse_benchmark_item(&dir_name, &content, &target_re, &run_opts_re) {
            benchmarks.push(item);
        }
    }

    benchmarks.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then(a.data_type.cmp(&b.data_type))
            .then(a.name.cmp(&b.name))
    });
    Ok(benchmarks)
}

fn parse_benchmark_item(
    dir_name: &str,
    cmake_content: &str,
    target_re: &Regex,
    run_opts_re: &Regex,
) -> Option<BenchmarkItem> {
    let target = target_re
        .captures(cmake_content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())?;

    let run_options = run_opts_re
        .captures(cmake_content)
        .and_then(|c| c.get(1))
        .map(|m| {
            m.as_str()
                .split_whitespace()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let (category, data_type) = split_category_type(dir_name);
    Some(BenchmarkItem {
        name: target,
        category,
        data_type,
        run_options,
    })
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

    #[test]
    fn parses_cmakelists_item() {
        let target_re =
            Regex::new(r"llvm_multisource\(\s*([^) \t\r\n]+)\s*\)").expect("valid target regex");
        let run_opts_re =
            Regex::new(r"set\(\s*RUN_OPTIONS\s+([^)]+)\)").expect("valid run options regex");
        let cmake = r#"
            list(APPEND LDFLAGS -lm)
            set(RUN_OPTIONS 9100 14)
            llvm_multisource(InductionVariable-dbl)
        "#;
        let item = parse_benchmark_item("InductionVariable-dbl", cmake, &target_re, &run_opts_re)
            .expect("item should parse");

        assert_eq!(item.name, "InductionVariable-dbl");
        assert_eq!(item.category, "InductionVariable");
        assert_eq!(item.data_type, "dbl");
        assert_eq!(item.run_options, vec!["9100", "14"]);
    }

    #[test]
    fn splits_unknown_type() {
        let (category, data_type) = split_category_type("SomeSuite");
        assert_eq!(category, "SomeSuite");
        assert_eq!(data_type, "unknown");
    }
}
