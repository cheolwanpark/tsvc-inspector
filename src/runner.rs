use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow};

use crate::error::AppResult;
use crate::model::{BenchmarkItem, BuildPurpose, CompilerConfig, FunctionRunMode};

#[derive(Clone, Debug)]
pub struct RunnerConfig {
    pub tsvc_root: PathBuf,
    pub clang: String,
    pub build_root: PathBuf,
    pub jobs: usize,
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
    compiler_config: &CompilerConfig,
    selected_function_symbol: &str,
    run_mode: FunctionRunMode,
    mut log: F,
) -> AppResult<RuntimeJobRawOutput>
where
    F: FnMut(String),
{
    fs::create_dir_all(&config.build_root)
        .with_context(|| format!("create build root {}", config.build_root.display()))?;

    let build_dir = build_dir_path(config, compiler_config, BuildPurpose::Runtime);
    let _ = run_build(
        config,
        &build_dir,
        benchmark,
        compiler_config,
        BuildPurpose::Runtime,
        None,
        &mut log,
    )?;

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
    compiler_config: &CompilerConfig,
    selected_function_symbol: &str,
    mut log: F,
) -> AppResult<AnalysisFastRawOutput>
where
    F: FnMut(String),
{
    fs::create_dir_all(&config.build_root)
        .with_context(|| format!("create build root {}", config.build_root.display()))?;

    let build_dir = build_dir_path(config, compiler_config, BuildPurpose::Analysis);
    let build_capture = run_build(
        config,
        &build_dir,
        benchmark,
        compiler_config,
        BuildPurpose::Analysis,
        Some(selected_function_symbol),
        &mut log,
    )?;
    let remark_file = locate_remark_file(&build_dir, &benchmark.name);

    Ok(AnalysisFastRawOutput {
        build_trace: format!("{}\n{}", build_capture.stdout, build_capture.stderr),
        remark_file,
    })
}

fn build_dir_path(
    config: &RunnerConfig,
    compiler_config: &CompilerConfig,
    purpose: BuildPurpose,
) -> PathBuf {
    let purpose_dir = match purpose {
        BuildPurpose::Runtime => "run",
        BuildPurpose::Analysis => "analysis",
    };
    config
        .build_root
        .join("build-tsvc-native")
        .join(purpose_dir)
        .join(compiler_config.config_id())
}

fn run_build<F>(
    config: &RunnerConfig,
    build_dir: &Path,
    benchmark: &BenchmarkItem,
    compiler_config: &CompilerConfig,
    purpose: BuildPurpose,
    selected_function_symbol: Option<&str>,
    log: &mut F,
) -> AppResult<CommandCapture>
where
    F: FnMut(String),
{
    let target_dir = target_dir(build_dir, &benchmark.name);
    fs::create_dir_all(&target_dir).with_context(|| format!("create {}", target_dir.display()))?;
    prepare_target_rebuild(&target_dir, &benchmark.name, log)?;
    log(format!(
        "build | native clang pipeline (jobs hint: {})",
        config.jobs.max(1)
    ));

    let source_dir = source_dir(&config.tsvc_root, &benchmark.name);
    let tsc_source = source_dir.join("tsc.c");
    let dummy_source = source_dir.join("dummy.c");
    if !tsc_source.exists() {
        return Err(anyhow!("missing source file: {}", tsc_source.display()));
    }
    if !dummy_source.exists() {
        return Err(anyhow!("missing source file: {}", dummy_source.display()));
    }

    let dummy_obj = target_dir.join("dummy.c.o");
    let tsc_obj = target_dir.join("tsc.c.o");
    let binary = target_dir.join(&benchmark.name);

    let mut combined_stdout = String::new();
    let mut combined_stderr = String::new();

    let runtime_flags = compiler_config.runtime_c_flags();
    let compile_tsc_flags = compile_tsc_flags(compiler_config, purpose, selected_function_symbol);
    let baseline = baseline_flags();
    let common_runtime = merge_flags(&runtime_flags, &baseline);
    let common_tsc = merge_flags(&compile_tsc_flags, &baseline);
    if let Some(filter_arg) = compile_tsc_flags
        .iter()
        .find(|arg| arg.starts_with("-filter-print-funcs="))
    {
        log(format!(
            "build | llvm changed-IR trace scoped via -mllvm {}",
            filter_arg
        ));
    }

    let dummy_capture = compile_source(
        &config.clang,
        &dummy_source,
        &dummy_obj,
        &common_runtime,
        log,
    )?;
    append_capture(&mut combined_stdout, &mut combined_stderr, dummy_capture);

    let tsc_capture = compile_source(&config.clang, &tsc_source, &tsc_obj, &common_tsc, log)?;
    append_capture(&mut combined_stdout, &mut combined_stderr, tsc_capture);

    let link_capture = link_target(
        &config.clang,
        &dummy_obj,
        &tsc_obj,
        &binary,
        &common_runtime,
        log,
    )?;
    append_capture(&mut combined_stdout, &mut combined_stderr, link_capture);

    Ok(CommandCapture {
        stdout: combined_stdout,
        stderr: combined_stderr,
    })
}

fn compile_source<F>(
    clang: &str,
    source: &Path,
    output: &Path,
    flags: &[String],
    log: &mut F,
) -> AppResult<CommandCapture>
where
    F: FnMut(String),
{
    let mut compile = Command::new(clang);
    for flag in flags {
        compile.arg(flag);
    }
    compile.arg("-c").arg(source).arg("-o").arg(output);
    capture_command(&mut compile, log)
}

fn link_target<F>(
    clang: &str,
    dummy_obj: &Path,
    tsc_obj: &Path,
    output: &Path,
    flags: &[String],
    log: &mut F,
) -> AppResult<CommandCapture>
where
    F: FnMut(String),
{
    let mut link = Command::new(clang);
    for flag in flags {
        link.arg(flag);
    }
    link.arg(dummy_obj)
        .arg(tsc_obj)
        .arg("-o")
        .arg(output)
        .arg("-lm");
    capture_command(&mut link, log)
}

fn merge_flags(primary: &[String], secondary: &[&str]) -> Vec<String> {
    let mut out = Vec::with_capacity(primary.len() + secondary.len());
    out.extend(primary.iter().cloned());
    out.extend(secondary.iter().map(|v| (*v).to_string()));
    out
}

fn baseline_flags() -> [&'static str; 4] {
    ["-DNDEBUG", "-std=gnu99", "-w", "-Werror=date-time"]
}

fn compile_tsc_flags(
    compiler_config: &CompilerConfig,
    purpose: BuildPurpose,
    selected_function_symbol: Option<&str>,
) -> Vec<String> {
    let mut flags = compiler_config.c_flags_for(purpose);
    if purpose != BuildPurpose::Analysis {
        return flags;
    }

    if !flags.iter().any(|flag| flag.starts_with("-print-changed")) {
        return flags;
    }

    let Some(symbol) = selected_function_symbol
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return flags;
    };

    flags.push(String::from("-mllvm"));
    flags.push(format!("-filter-print-funcs={symbol}"));
    flags
}

fn source_dir(tsvc_root: &Path, benchmark: &str) -> PathBuf {
    tsvc_root
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC")
        .join(benchmark)
}

fn target_dir(build_dir: &Path, benchmark: &str) -> PathBuf {
    build_dir
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC")
        .join(benchmark)
}

fn append_capture(stdout: &mut String, stderr: &mut String, capture: CommandCapture) {
    if !stdout.is_empty() {
        stdout.push('\n');
    }
    stdout.push_str(&capture.stdout);
    if !stderr.is_empty() {
        stderr.push('\n');
    }
    stderr.push_str(&capture.stderr);
}

fn prepare_target_rebuild<F>(target_dir: &Path, target: &str, log: &mut F) -> AppResult<()>
where
    F: FnMut(String),
{
    if !target_dir.exists() {
        return Ok(());
    }
    remove_target_outputs(target_dir, log)?;

    let binary = target_dir.join(target);
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
        let should_remove = matches!(ext, "o" | "obj" | "bc" | "ll" | "s" | "yaml")
            || file_name.ends_with(".opt.yaml");
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

pub fn benchmark_binary_path(build_dir: &Path, benchmark: &str) -> PathBuf {
    target_dir(build_dir, benchmark).join(benchmark)
}

fn locate_remark_file(build_dir: &Path, benchmark: &str) -> Option<PathBuf> {
    let target_dir = target_dir(build_dir, benchmark);
    let primary = target_dir.join("tsc.c.opt.yaml");
    if primary.exists() {
        return Some(primary);
    }
    let secondary = target_dir.join("tsc.opt.yaml");
    if secondary.exists() {
        return Some(secondary);
    }
    find_first_opt_yaml(&target_dir)
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
    fn benchmark_path_contains_target_name() {
        let p = benchmark_binary_path(Path::new("/tmp/build"), "InductionVariable-dbl");
        assert!(p.ends_with("InductionVariable-dbl/InductionVariable-dbl"));
    }

    #[test]
    fn render_command_with_native_clang() {
        let mut compile = Command::new("clang");
        compile
            .arg("-O3")
            .arg("-c")
            .arg("tsc.c")
            .arg("-o")
            .arg("tsc.c.o");
        let rendered = render_command(&compile);
        assert!(rendered.contains("clang -O3 -c tsc.c -o tsc.c.o"));
    }

    #[test]
    fn prepare_target_rebuild_removes_native_artifacts() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tsvc-tui-prepare-rebuild-test-{unique}"));
        let target_dir = root.join("Foo");
        fs::create_dir_all(&target_dir).expect("create target dir");
        fs::write(target_dir.join("build.make"), "keep").expect("write build.make");
        fs::write(target_dir.join("tsc.c.o"), "obj").expect("write object");
        fs::write(target_dir.join("tsc.c.opt.yaml"), "yaml").expect("write remark");
        fs::write(target_dir.join("Foo"), "bin").expect("write binary");

        let mut logs = Vec::new();
        prepare_target_rebuild(&target_dir, "Foo", &mut |line| logs.push(line)).expect("prepare");
        assert!(target_dir.join("build.make").exists());
        assert!(!target_dir.join("tsc.c.o").exists());
        assert!(!target_dir.join("tsc.c.opt.yaml").exists());
        assert!(!target_dir.join("Foo").exists());
        assert!(logs.iter().any(|line| line.contains("removed")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn config_build_dir_contains_config_id() {
        let config = CompilerConfig::default();
        let runner = RunnerConfig {
            tsvc_root: PathBuf::from("/tmp/tsvc"),
            clang: String::from("clang"),
            build_root: PathBuf::from("/tmp/build"),
            jobs: 1,
        };
        let run_dir = build_dir_path(&runner, &config, BuildPurpose::Runtime);
        assert!(run_dir.ends_with(format!("build-tsvc-native/run/{}", config.config_id())));
    }

    #[test]
    fn compile_tsc_flags_scopes_print_changed_to_selected_function() {
        let config = CompilerConfig::default();
        let flags = compile_tsc_flags(&config, BuildPurpose::Analysis, Some("s161"));
        assert!(flags.iter().any(|flag| flag == "-print-changed"));
        assert!(flags.iter().any(|flag| flag == "-filter-print-funcs=s161"));
    }

    #[test]
    fn compile_tsc_flags_skip_filter_without_print_changed() {
        let config = CompilerConfig {
            emit_print_changed: false,
            ..CompilerConfig::default()
        };
        let flags = compile_tsc_flags(&config, BuildPurpose::Analysis, Some("s161"));
        assert!(
            !flags
                .iter()
                .any(|flag| flag.starts_with("-filter-print-funcs="))
        );
    }
}
