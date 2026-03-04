#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchmarkManifestEntry {
    pub name: &'static str,
    pub run_options: &'static [&'static str],
}

pub const BENCHMARK_MANIFEST: &[BenchmarkManifestEntry] = &[
    BenchmarkManifestEntry {
        name: "ControlFlow-dbl",
        run_options: &["2325", "14"],
    },
    BenchmarkManifestEntry {
        name: "ControlFlow-flt",
        run_options: &["2325", "5"],
    },
    BenchmarkManifestEntry {
        name: "ControlLoops-dbl",
        run_options: &["1640", "14"],
    },
    BenchmarkManifestEntry {
        name: "ControlLoops-flt",
        run_options: &["1640", "5"],
    },
    BenchmarkManifestEntry {
        name: "CrossingThresholds-dbl",
        run_options: &["5880", "14"],
    },
    BenchmarkManifestEntry {
        name: "CrossingThresholds-flt",
        run_options: &["5880", "5"],
    },
    BenchmarkManifestEntry {
        name: "Equivalencing-dbl",
        run_options: &["3125", "14"],
    },
    BenchmarkManifestEntry {
        name: "Equivalencing-flt",
        run_options: &["3125", "5"],
    },
    BenchmarkManifestEntry {
        name: "Expansion-dbl",
        run_options: &["4160", "14"],
    },
    BenchmarkManifestEntry {
        name: "Expansion-flt",
        run_options: &["4160", "5"],
    },
    BenchmarkManifestEntry {
        name: "GlobalDataFlow-dbl",
        run_options: &["3450", "14"],
    },
    BenchmarkManifestEntry {
        name: "GlobalDataFlow-flt",
        run_options: &["3450", "5"],
    },
    BenchmarkManifestEntry {
        name: "IndirectAddressing-dbl",
        run_options: &["12500", "14"],
    },
    BenchmarkManifestEntry {
        name: "IndirectAddressing-flt",
        run_options: &["12500", "5"],
    },
    BenchmarkManifestEntry {
        name: "InductionVariable-dbl",
        run_options: &["9100", "14"],
    },
    BenchmarkManifestEntry {
        name: "InductionVariable-flt",
        run_options: &["9100", "5"],
    },
    BenchmarkManifestEntry {
        name: "LinearDependence-dbl",
        run_options: &["3570", "14"],
    },
    BenchmarkManifestEntry {
        name: "LinearDependence-flt",
        run_options: &["3570", "5"],
    },
    BenchmarkManifestEntry {
        name: "LoopRerolling-dbl",
        run_options: &["5260", "14"],
    },
    BenchmarkManifestEntry {
        name: "LoopRerolling-flt",
        run_options: &["5260", "5"],
    },
    BenchmarkManifestEntry {
        name: "LoopRestructuring-dbl",
        run_options: &["4350", "14"],
    },
    BenchmarkManifestEntry {
        name: "LoopRestructuring-flt",
        run_options: &["4350", "5"],
    },
    BenchmarkManifestEntry {
        name: "NodeSplitting-dbl",
        run_options: &["10000", "14"],
    },
    BenchmarkManifestEntry {
        name: "NodeSplitting-flt",
        run_options: &["10000", "5"],
    },
    BenchmarkManifestEntry {
        name: "Packing-dbl",
        run_options: &["50000", "14"],
    },
    BenchmarkManifestEntry {
        name: "Packing-flt",
        run_options: &["50000", "5"],
    },
    BenchmarkManifestEntry {
        name: "Recurrences-dbl",
        run_options: &["20000", "14"],
    },
    BenchmarkManifestEntry {
        name: "Recurrences-flt",
        run_options: &["20000", "5"],
    },
    BenchmarkManifestEntry {
        name: "Reductions-dbl",
        run_options: &["1670", "14"],
    },
    BenchmarkManifestEntry {
        name: "Reductions-flt",
        run_options: &["1670", "5"],
    },
    BenchmarkManifestEntry {
        name: "Searching-dbl",
        run_options: &["80000", "14"],
    },
    BenchmarkManifestEntry {
        name: "Searching-flt",
        run_options: &["80000", "5"],
    },
    BenchmarkManifestEntry {
        name: "StatementReordering-dbl",
        run_options: &["20000", "14"],
    },
    BenchmarkManifestEntry {
        name: "StatementReordering-flt",
        run_options: &["20000", "5"],
    },
    BenchmarkManifestEntry {
        name: "Symbolics-dbl",
        run_options: &["9090", "14"],
    },
    BenchmarkManifestEntry {
        name: "Symbolics-flt",
        run_options: &["9090", "5"],
    },
];

#[cfg(test)]
pub fn find(name: &str) -> Option<&'static BenchmarkManifestEntry> {
    BENCHMARK_MANIFEST.iter().find(|entry| entry.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn benchmark_names_are_unique() {
        let mut seen = HashSet::new();
        for entry in BENCHMARK_MANIFEST {
            assert!(
                seen.insert(entry.name),
                "duplicate benchmark entry: {}",
                entry.name
            );
        }
    }

    #[test]
    fn run_options_are_non_empty() {
        for entry in BENCHMARK_MANIFEST {
            assert!(
                !entry.run_options.is_empty(),
                "missing run options for {}",
                entry.name
            );
        }
    }
}
