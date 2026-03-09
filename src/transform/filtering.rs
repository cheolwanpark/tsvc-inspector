use crate::core::model::{BenchmarkFunction, RemarkEntry};

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
