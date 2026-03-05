use crate::core::model::{BenchmarkFunction, LoopResult, RemarkEntry};

pub fn filter_loop_results_for_selected_function(
    loop_results: Vec<LoopResult>,
    selected_function: &BenchmarkFunction,
) -> Vec<LoopResult> {
    loop_results
        .into_iter()
        .filter(|entry| {
            entry
                .loop_id
                .eq_ignore_ascii_case(&selected_function.loop_id)
        })
        .collect::<Vec<_>>()
}

pub fn filter_remarks_for_selected_function(
    remarks: Vec<RemarkEntry>,
    selected_function: &BenchmarkFunction,
) -> Vec<RemarkEntry> {
    remarks
        .into_iter()
        .filter(|remark| {
            remark
                .function
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case(&selected_function.symbol))
        })
        .collect::<Vec<_>>()
}
