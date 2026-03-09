pub const BENCHMARK_NAMES: &[&str] = &[
    "ControlFlow-dbl",
    "ControlFlow-flt",
    "ControlLoops-dbl",
    "ControlLoops-flt",
    "CrossingThresholds-dbl",
    "CrossingThresholds-flt",
    "Equivalencing-dbl",
    "Equivalencing-flt",
    "Expansion-dbl",
    "Expansion-flt",
    "GlobalDataFlow-dbl",
    "GlobalDataFlow-flt",
    "IndirectAddressing-dbl",
    "IndirectAddressing-flt",
    "InductionVariable-dbl",
    "InductionVariable-flt",
    "LinearDependence-dbl",
    "LinearDependence-flt",
    "LoopRerolling-dbl",
    "LoopRerolling-flt",
    "LoopRestructuring-dbl",
    "LoopRestructuring-flt",
    "NodeSplitting-dbl",
    "NodeSplitting-flt",
    "Packing-dbl",
    "Packing-flt",
    "Recurrences-dbl",
    "Recurrences-flt",
    "Reductions-dbl",
    "Reductions-flt",
    "Searching-dbl",
    "Searching-flt",
    "StatementReordering-dbl",
    "StatementReordering-flt",
    "Symbolics-dbl",
    "Symbolics-flt",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_names_are_non_empty() {
        assert!(!BENCHMARK_NAMES.is_empty());
        assert!(BENCHMARK_NAMES.iter().all(|name| !name.is_empty()));
    }
}
