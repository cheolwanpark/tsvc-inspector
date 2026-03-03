use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use regex::Regex;

use crate::error::AppResult;
use crate::model::{BenchmarkItem, BuildPurpose, CompileProfile, FunctionRunMode, JobKind};
use crate::parser::IrSnapshot;

#[derive(Clone, Debug)]
pub struct RunnerConfig {
    pub tsvc_root: PathBuf,
    pub clang: String,
    pub cmake: String,
    pub opt: String,
    pub build_root: PathBuf,
    pub jobs: usize,
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

#[derive(Debug)]
pub struct AnalysisDeepRequest<'a> {
    pub selected_function_symbol: &'a str,
    pub target_pass_key: &'a str,
    pub target_pass_occurrence: usize,
}

#[derive(Debug)]
pub struct AnalysisDeepRawOutput {
    pub snapshots: Vec<IrSnapshot>,
    pub remark_file: Option<PathBuf>,
    pub window_start: usize,
    pub window_end: usize,
    pub mapped_index: Option<usize>,
}

#[derive(Clone, Debug)]
struct BisectPassMeta {
    index: usize,
    pass: String,
    target: String,
    pass_key: String,
    pass_occurrence: usize,
}

pub fn execute_runtime_job<F>(
    config: &RunnerConfig,
    benchmark: &BenchmarkItem,
    profile: CompileProfile,
    kind: JobKind,
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

    if matches!(kind, JobKind::Build | JobKind::BuildAndRun) {
        run_configure(config, &build_dir, profile, BuildPurpose::Runtime, &mut log)?;
        let _ = run_build(config, &build_dir, &benchmark.name, &mut log)?;
    }

    let mut run_stdout = String::new();
    if matches!(kind, JobKind::Run | JobKind::BuildAndRun) {
        let binary = benchmark_binary_path(&build_dir, &benchmark.name);
        if !binary.exists() {
            return Err(anyhow!(
                "target binary not found: {} (build it first with 'b')",
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
        run_stdout = output.stdout;
    }

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

pub fn execute_analysis_deep<F>(
    config: &RunnerConfig,
    benchmark: &BenchmarkItem,
    profile: CompileProfile,
    request: AnalysisDeepRequest<'_>,
    mut log: F,
) -> AppResult<AnalysisDeepRawOutput>
where
    F: FnMut(String),
{
    fs::create_dir_all(&config.build_root)
        .with_context(|| format!("create build root {}", config.build_root.display()))?;

    let tsvc_source = config
        .tsvc_root
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC")
        .join(&benchmark.name)
        .join("tsc.c");
    if !tsvc_source.is_file() {
        return Err(anyhow!(
            "source not found for deep analysis: {}",
            tsvc_source.display()
        ));
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let deep_dir = config
        .build_root
        .join(format!("deep-analysis-{}-{nonce}", benchmark.name));
    fs::create_dir_all(&deep_dir).with_context(|| format!("create {}", deep_dir.display()))?;

    let deep_outcome = (|| -> AppResult<AnalysisDeepRawOutput> {
        log(format!(
            "deep analyze | benchmark={} function={} pass_key={} occurrence={}",
            benchmark.name,
            request.selected_function_symbol,
            request.target_pass_key,
            request.target_pass_occurrence
        ));
        let bc_path = deep_dir.join("tsc.O0.dbg.bc");
        let mut compile = Command::new(&config.clang);
        compile
            .arg("-O0")
            .arg("-g")
            .arg("-fno-discard-value-names")
            .arg("-Xclang")
            .arg("-disable-O0-optnone")
            .arg("-emit-llvm")
            .arg("-c")
            .arg(&tsvc_source)
            .arg("-o")
            .arg(&bc_path);
        let _ = capture_command(&mut compile, &mut log)?;

        let mut bisect_probe = Command::new(&config.opt);
        bisect_probe
            .arg("-passes=default<O3>")
            .arg("-opt-bisect-limit=-1")
            .arg("-disable-output")
            .arg(&bc_path);
        let bisect_probe_capture = capture_command(&mut bisect_probe, &mut log)?;
        let bisect_log = format!(
            "{}\n{}",
            bisect_probe_capture.stderr, bisect_probe_capture.stdout
        );
        let passes = parse_bisect_passes(&bisect_log);
        let max_pass_idx = passes.iter().map(|m| m.index).max().unwrap_or(0);

        let mapped_index = passes
            .iter()
            .find(|meta| {
                meta.pass_key == request.target_pass_key
                    && meta.pass_occurrence == request.target_pass_occurrence
            })
            .map(|meta| meta.index);

        let (window_start, window_end) = if let Some(center) = mapped_index {
            (
                center.saturating_sub(config.analysis_window),
                center
                    .saturating_add(config.analysis_window)
                    .min(max_pass_idx),
            )
        } else {
            (
                0,
                config.analysis_window.saturating_mul(2).min(max_pass_idx),
            )
        };

        let pass_by_index = passes
            .iter()
            .map(|meta| (meta.index, meta))
            .collect::<HashMap<_, _>>();

        let mut snapshots = Vec::new();
        for raw_idx in window_start..=window_end {
            let step_path = deep_dir.join(format!("step-{raw_idx:05}.ll"));
            let mut opt_step = Command::new(&config.opt);
            opt_step
                .arg("-passes=default<O3>")
                .arg(format!("-opt-bisect-limit={raw_idx}"))
                .arg("-S")
                .arg(&bc_path)
                .arg("-o")
                .arg(&step_path);
            let _ = capture_command(&mut opt_step, &mut log)?;
            let snapshot = fs::read_to_string(&step_path)
                .with_context(|| format!("read {}", step_path.display()))?;

            let (pass, target, pass_occurrence) = if raw_idx == 0 {
                (
                    String::from("(initial IR)"),
                    String::from("[module]"),
                    1usize,
                )
            } else if let Some(meta) = pass_by_index.get(&raw_idx) {
                (
                    meta.pass.clone(),
                    meta.target.clone(),
                    meta.pass_occurrence.max(1),
                )
            } else {
                (
                    format!("(pass-{raw_idx})"),
                    String::from("[module]"),
                    1usize,
                )
            };

            snapshots.push(IrSnapshot {
                raw_index: raw_idx,
                pass,
                pass_occurrence,
                target,
                snapshot,
            });
        }

        let analysis_build_dir = build_dir_path(config, profile, BuildPurpose::Analysis);
        let remark_file = locate_remark_file(&analysis_build_dir, &benchmark.name);

        Ok(AnalysisDeepRawOutput {
            snapshots,
            remark_file,
            window_start,
            window_end,
            mapped_index,
        })
    })();

    let _ = fs::remove_dir_all(&deep_dir);
    deep_outcome
}

fn parse_bisect_passes(log_text: &str) -> Vec<BisectPassMeta> {
    let line_re =
        Regex::new(r"^BISECT:\s+running pass \((\d+)\)\s+(.+?)\s+on\s+(.+)$").expect("regex");
    let mut metas = Vec::new();
    let mut occurrence_by_pass = HashMap::<String, usize>::new();

    for line in log_text.lines() {
        let Some(caps) = line_re.captures(line) else {
            continue;
        };
        let index = caps
            .get(1)
            .and_then(|m| m.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        if index == 0 {
            continue;
        }
        let pass = caps
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let target = caps
            .get(3)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| String::from("[module]"));
        let pass_key = normalize_pass_key(&pass);
        let pass_occurrence = {
            let next = occurrence_by_pass.get(&pass_key).copied().unwrap_or(0) + 1;
            occurrence_by_pass.insert(pass_key.clone(), next);
            next
        };

        metas.push(BisectPassMeta {
            index,
            pass,
            target,
            pass_key,
            pass_occurrence,
        });
    }

    metas.sort_by_key(|m| m.index);
    metas
}

fn normalize_pass_key(pass: &str) -> String {
    let lowercase = pass.to_ascii_lowercase();
    let without_suffix = lowercase.strip_suffix("pass").unwrap_or(&lowercase);
    without_suffix
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
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

    #[test]
    fn parse_bisect_lines_extracts_occurrence() {
        let text = r#"
BISECT: running pass (1) SROAPass on foo
BISECT: running pass (2) SROAPass on bar
BISECT: running pass (3) LICMPass on loop %x in function foo
"#;
        let pass = parse_bisect_passes(text);
        assert_eq!(pass.len(), 3);
        assert_eq!(pass[0].pass_occurrence, 1);
        assert_eq!(pass[1].pass_occurrence, 2);
        assert_eq!(pass[2].pass_key, "licm");
    }
}
