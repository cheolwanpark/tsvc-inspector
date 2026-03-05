use crate::core::model::BenchmarkItem;
use crate::data::discovery::RawBenchmark;
use crate::transform::source::build_kernel_focused_source_and_functions;

pub fn build_benchmark_catalog(raw_benchmarks: Vec<RawBenchmark>) -> Vec<BenchmarkItem> {
    raw_benchmarks
        .into_iter()
        .map(|raw| {
            let (source_code, available_functions) = build_kernel_focused_source_and_functions(
                &raw.tsc_source,
                raw.tsc_inc_source.as_deref(),
            );

            BenchmarkItem {
                name: raw.name,
                category: raw.category,
                data_type: raw.data_type,
                run_options: raw.run_options,
                available_functions,
                source_code,
            }
        })
        .collect()
}
