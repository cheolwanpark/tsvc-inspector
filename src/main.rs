use std::path::PathBuf;

use clap::Parser;

use tsvc_inspector::AppResult;
use tsvc_inspector::display::runtime::{self, RuntimeOptions};

#[derive(Parser, Debug)]
#[command(
    name = "tsvc-inspector",
    version,
    about = "TSVC inspector with Ratatui"
)]
struct Cli {
    #[arg(long, default_value = "llvm-test-suite")]
    tsvc_root: PathBuf,

    #[arg(long, default_value = "clang")]
    clang: String,

    #[arg(long, default_value = ".")]
    build_root: PathBuf,

    #[arg(long, default_value_t = default_jobs())]
    jobs: usize,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn main() -> AppResult<()> {
    let cli = Cli::parse();
    runtime::run(RuntimeOptions {
        tsvc_root: cli.tsvc_root,
        clang: cli.clang,
        build_root: cli.build_root,
        jobs: cli.jobs,
    })
}
