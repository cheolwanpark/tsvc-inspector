use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow};

use crate::error::AppResult;
use crate::model::{BenchmarkItem, BuildPurpose, CompileProfile, FunctionRunMode};

#[derive(Clone, Debug)]
pub struct RunnerConfig {
    pub tsvc_root: PathBuf,
    pub clang: String,
    pub cmake: String,
    #[allow(dead_code)]
    pub opt: String,
    pub build_root: PathBuf,
    pub jobs: usize,
    #[allow(dead_code)]
    pub analysis_window: usize,
}

#[derive(Debug)]
pub struct RuntimeJobRawOutput {
    pub run_stdout: String,
    pub remark_file: Option<PathBuf>,
}

#[derive(Debug)]
pub struct AnalysisFastRawOutput {
    pub build_trace: String,
    pub remark_file: Option<PathBuf>,
}

pub fn execute_runtime_job<F>(
    config: &RunnerConfig,
    benchmark: &BenchmarkItem,
    profile: CompileProfile,
    selected_function_symbol: &str,
    run_mode: FunctionRunMode,
    mut log: F,
) -> AppResult<RuntimeJobRawOutput>
where
    F: FnMut(String),
{
    fs::create_dir_all(&config.build_root)
        .with_context(|| format!("create build root {}", config.build_root.display()))?;

    let build_dir = build_dir_path(config, profile, BuildPurpose::Runtime);

    run_configure(config, &build_dir, profile, BuildPurpose::Runtime, &mut log)?;
    let _ = run_build(config, &build_dir, &benchmark.name, &mut log)?;

    let binary = benchmark_binary_path(&build_dir, &benchmark.name);
    if !binary.exists() {
        return Err(anyhow!(
            "target binary not found: {} (build failed)",
            binary.display()
        ));
    }

    let mut run = Command::new(&binary);
    run.args(&benchmark.run_options);
    if run_mode == FunctionRunMode::RealSelective {
        log(format!(
            "env | TSVC_TUI_FUNCTION_FILTER={selected_function_symbol}"
        ));
        run.env("TSVC_TUI_FUNCTION_FILTER", selected_function_symbol);
    }
    let output = capture_command(&mut run, &mut log)?;
    let run_stdout = output.stdout;

    let remark_file = locate_remark_file(&build_dir, &benchmark.name);
    Ok(RuntimeJobRawOutput {
        run_stdout,
        remark_file,
    })
}

pub fn execute_analysis_fast<F>(
    config: &RunnerConfig,
    benchmark: &BenchmarkItem,
    profile: CompileProfile,
    mut log: F,
) -> AppResult<AnalysisFastRawOutput>
where
    F: FnMut(String),
{
    fs::create_dir_all(&config.build_root)
        .with_context(|| format!("create build root {}", config.build_root.display()))?;

    let build_dir = build_dir_path(config, profile, BuildPurpose::Analysis);
    run_configure(
        config,
        &build_dir,
        profile,
        BuildPurpose::Analysis,
        &mut log,
    )?;
    let build_capture = run_build(config, &build_dir, &benchmark.name, &mut log)?;
    let remark_file = locate_remark_file(&build_dir, &benchmark.name);

    Ok(AnalysisFastRawOutput {
        build_trace: format!("{}\n{}", build_capture.stdout, build_capture.stderr),
        remark_file,
    })
}

fn build_dir_path(
    config: &RunnerConfig,
    profile: CompileProfile,
    purpose: BuildPurpose,
) -> PathBuf {
    config.build_root.join(profile.build_dir_name_for(purpose))
}

fn run_configure<F>(
    config: &RunnerConfig,
    build_dir: &Path,
    profile: CompileProfile,
    purpose: BuildPurpose,
    log: &mut F,
) -> AppResult<()>
where
    F: FnMut(String),
{
    let mut configure = Command::new(&config.cmake);
    configure
        .arg("-S")
        .arg(&config.tsvc_root)
        .arg("-B")
        .arg(build_dir)
        .arg(format!("-DCMAKE_C_COMPILER={}", config.clang))
        .arg("-DTEST_SUITE_SUBDIRS=MultiSource/Benchmarks/TSVC")
        .arg("-DTEST_SUITE_BENCHMARKING_ONLY=ON")
        .arg("-DTEST_SUITE_RUN_BENCHMARKS=OFF")
        .arg(format!("-DCMAKE_C_FLAGS={}", profile.c_flags_for(purpose)));

    let _ = capture_command(&mut configure, log)?;
    Ok(())
}

fn run_build<F>(
    config: &RunnerConfig,
    build_dir: &Path,
    target: &str,
    log: &mut F,
) -> AppResult<CommandCapture>
where
    F: FnMut(String),
{
    prepare_target_rebuild(build_dir, target, log)?;

    let jobs = effective_build_jobs(config.jobs);
    let mut build = Command::new(&config.cmake);
    build
        .arg("--build")
        .arg(build_dir)
        .arg("--target")
        .arg(target)
        .arg("-j")
        .arg(jobs.to_string());

    let capture = capture_command(&mut build, log)?;
    Ok(capture)
}

fn prepare_target_rebuild<F>(build_dir: &Path, target: &str, log: &mut F) -> AppResult<()>
where
    F: FnMut(String),
{
    let target_dir = build_dir
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC")
        .join(target);
    let obj_dir = target_dir.join("CMakeFiles").join(format!("{target}.dir"));
    let binary = target_dir.join(target);

    if obj_dir.is_dir() {
        remove_target_outputs(&obj_dir, log)?;
    }
    if binary.exists() {
        fs::remove_file(&binary).with_context(|| format!("remove {}", binary.display()))?;
        log(format!("prep | removed file {}", binary.display()));
    }

    Ok(())
}

fn remove_target_outputs<F>(dir: &Path, log: &mut F) -> AppResult<()>
where
    F: FnMut(String),
{
    let entries = fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            remove_target_outputs(&path, log)?;
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        let ext = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        let should_remove =
            matches!(ext, "o" | "obj" | "bc" | "ll" | "s") || file_name.ends_with(".opt.yaml");
        if !should_remove {
            continue;
        }

        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        log(format!("prep | removed file {}", path.display()));
    }
    Ok(())
}

#[derive(Debug)]
struct CommandCapture {
    stdout: String,
    stderr: String,
}

fn capture_command<F>(command: &mut Command, log: &mut F) -> AppResult<CommandCapture>
where
    F: FnMut(String),
{
    let rendered = render_command(command);
    log(format!("$ {rendered}"));
    let output = command
        .output()
        .with_context(|| format!("failed to execute: {rendered}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    for line in stdout.lines() {
        log(format!("stdout | {line}"));
    }
    for line in stderr.lines() {
        log(format!("stderr | {line}"));
    }

    if !output.status.success() {
        return Err(anyhow!(
            "command failed with status {}: {}",
            output.status,
            rendered
        ));
    }

    Ok(CommandCapture { stdout, stderr })
}

fn render_command(command: &Command) -> String {
    let mut rendered = command.get_program().to_string_lossy().to_string();
    for arg in command.get_args() {
        rendered.push(' ');
        rendered.push_str(&shell_escape(arg));
    }
    rendered
}

fn shell_escape(text: &OsStr) -> String {
    let s = text.to_string_lossy();
    if s.contains(' ') || s.contains('\t') {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

fn effective_build_jobs(configured_jobs: usize) -> usize {
    configured_jobs.max(1)
}

pub fn benchmark_binary_path(build_dir: &Path, benchmark: &str) -> PathBuf {
    build_dir
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC")
        .join(benchmark)
        .join(benchmark)
}

fn locate_remark_file(build_dir: &Path, benchmark: &str) -> Option<PathBuf> {
    let primary = build_dir
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC")
        .join(benchmark)
        .join("CMakeFiles")
        .join(format!("{benchmark}.dir"))
        .join("tsc.c.opt.yaml");
    if primary.exists() {
        return Some(primary);
    }

    let search_root = build_dir
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC")
        .join(benchmark);
    find_first_opt_yaml(&search_root)
}

fn find_first_opt_yaml(root: &Path) -> Option<PathBuf> {
    if !root.exists() {
        return None;
    }

    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let entries = match fs::read_dir(&path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().and_then(|x| x.to_str()) == Some("yaml")
                && p.file_name()
                    .and_then(|x| x.to_str())
                    .is_some_and(|name| name.ends_with(".opt.yaml"))
            {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_build_dir_names_are_stable() {
        assert_eq!(
            CompileProfile::O3Remarks.build_dir_name_for(BuildPurpose::Analysis),
            "build-tsvc-o3-remarks-analysis"
        );
        assert_eq!(
            CompileProfile::O3NoVec.build_dir_name_for(BuildPurpose::Analysis),
            "build-tsvc-o3-novec-analysis"
        );
        assert_eq!(
            CompileProfile::O3Default.build_dir_name_for(BuildPurpose::Runtime),
            "build-tsvc-o3-default-run"
        );
    }

    #[test]
    fn benchmark_path_contains_target_name() {
        let p = benchmark_binary_path(Path::new("/tmp/build"), "InductionVariable-dbl");
        assert!(p.ends_with("InductionVariable-dbl/InductionVariable-dbl"));
    }

    #[test]
    fn build_jobs_respect_requested_parallelism() {
        assert_eq!(effective_build_jobs(8), 8);
        assert_eq!(effective_build_jobs(1), 1);
        assert_eq!(effective_build_jobs(0), 1);
    }

    #[test]
    fn render_command_for_build_omits_clean_first() {
        let mut build = Command::new("cmake");
        build
            .arg("--build")
            .arg("/tmp/build")
            .arg("--target")
            .arg("foo")
            .arg("-j")
            .arg("1");
        let rendered = render_command(&build);
        assert!(!rendered.contains("--clean-first"));
    }

    #[test]
    fn prepare_target_rebuild_removes_target_artifacts() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tsvc-tui-prepare-rebuild-test-{unique}"));
        let obj_dir = root
            .join("MultiSource")
            .join("Benchmarks")
            .join("TSVC")
            .join("Foo")
            .join("CMakeFiles")
            .join("Foo.dir");
        fs::create_dir_all(&obj_dir).expect("create obj dir");
        fs::write(obj_dir.join("build.make"), "keep").expect("write build.make");
        fs::write(obj_dir.join("tsc.c.o"), "obj").expect("write object");
        fs::write(obj_dir.join("tsc.c.opt.yaml"), "yaml").expect("write remark");

        let binary = root
            .join("MultiSource")
            .join("Benchmarks")
            .join("TSVC")
            .join("Foo")
            .join("Foo");
        fs::create_dir_all(binary.parent().expect("binary parent")).expect("create target dir");
        fs::write(&binary, "bin").expect("write binary");

        let mut logs = Vec::new();
        prepare_target_rebuild(&root, "Foo", &mut |line| logs.push(line)).expect("prepare");
        assert!(obj_dir.exists());
        assert!(obj_dir.join("build.make").exists());
        assert!(!obj_dir.join("tsc.c.o").exists());
        assert!(!obj_dir.join("tsc.c.opt.yaml").exists());
        assert!(!binary.exists());
        assert!(logs.iter().any(|line| line.contains("removed")));

        let _ = fs::remove_dir_all(root);
    }
}
