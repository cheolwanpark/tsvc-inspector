use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};

use crate::core::error::AppResult;
use crate::data::manifest::BENCHMARK_NAMES;

#[derive(Clone, Debug)]
pub struct RawBenchmark {
    pub name: String,
    pub category: String,
    pub data_type: String,
    pub benchmark_dir: PathBuf,
    pub tsc_source: String,
    pub tsc_inc_source: Option<String>,
}

pub fn discover_raw_benchmarks(tsvc_root: &Path) -> AppResult<Vec<RawBenchmark>> {
    let tsvc_dir = tsvc_root
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC");
    if !tsvc_dir.is_dir() {
        return Err(anyhow!("TSVC directory not found: {}", tsvc_dir.display()));
    }

    let mut benchmarks = Vec::new();
    let tsc_inc_path = tsvc_dir.join("tsc.inc");
    let shared_tsc_inc = fs::read_to_string(&tsc_inc_path).ok();

    for name in BENCHMARK_NAMES {
        let benchmark_dir = tsvc_dir.join(name);
        if !benchmark_dir.is_dir() {
            continue;
        }

        let tsc_source_path = benchmark_dir.join("tsc.c");
        let tsc_source = fs::read_to_string(&tsc_source_path)
            .with_context(|| format!("read {}", tsc_source_path.display()))?;

        let (category, data_type) = split_category_type(name);
        benchmarks.push(RawBenchmark {
            name: (*name).to_string(),
            category,
            data_type,
            benchmark_dir,
            tsc_source,
            tsc_inc_source: shared_tsc_inc.clone(),
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
    fn splits_unknown_type() {
        let (category, data_type) = split_category_type("SomeSuite");
        assert_eq!(category, "SomeSuite");
        assert_eq!(data_type, "unknown");
    }
}
