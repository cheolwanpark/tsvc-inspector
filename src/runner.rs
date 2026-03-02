use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow};

use crate::error::AppResult;
use crate::model::{BenchmarkItem, CompileProfile, JobKind};

#[derive(Clone, Debug)]
pub struct RunnerConfig {
    pub tsvc_root: PathBuf,
    pub clang: String,
    pub cmake: String,
    pub build_root: PathBuf,
    pub jobs: usize,
}

#[derive(Debug)]
pub struct JobRawOutput {
    pub run_stdout: String,
    pub remark_file: Option<PathBuf>,
    pub build_trace: String,
}

pub fn execute_job<F>(
    config: &RunnerConfig,
    benchmark: &BenchmarkItem,
    profile: CompileProfile,
    kind: JobKind,
    mut log: F,
) -> AppResult<JobRawOutput>
where
    F: FnMut(String),
{
    fs::create_dir_all(&config.build_root)
        .with_context(|| format!("create build root {}", config.build_root.display()))?;

    let build_dir = config.build_root.join(profile.build_dir_name());
    let mut build_trace = String::new();

    if matches!(kind, JobKind::Build | JobKind::BuildAndRun) {
        run_configure(config, &build_dir, profile, &mut log)?;
        let build_capture = run_build(config, &build_dir, &benchmark.name, profile, &mut log)?;
        build_trace = format!("{}\n{}", build_capture.stdout, build_capture.stderr);
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
        let output = capture_command(&mut run, &mut log)?;
        run_stdout = output.stdout;
    }

    let remark_file = locate_remark_file(&build_dir, &benchmark.name);
    Ok(JobRawOutput {
        run_stdout,
        remark_file,
        build_trace,
    })
}

fn run_configure<F>(
    config: &RunnerConfig,
    build_dir: &Path,
    profile: CompileProfile,
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
        .arg(format!("-DCMAKE_C_FLAGS={}", profile.c_flags()));

    let _ = capture_command(&mut configure, log)?;
    Ok(())
}

fn run_build<F>(
    config: &RunnerConfig,
    build_dir: &Path,
    target: &str,
    profile: CompileProfile,
    log: &mut F,
) -> AppResult<CommandCapture>
where
    F: FnMut(String),
{
    let jobs = effective_build_jobs(config.jobs, profile);
    let mut build = Command::new(&config.cmake);
    build
        .arg("--build")
        .arg(build_dir)
        .arg("--clean-first")
        .arg("--target")
        .arg(target)
        .arg("-j")
        .arg(jobs.to_string());

    let capture = capture_command(&mut build, log)?;
    Ok(capture)
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

fn effective_build_jobs(configured_jobs: usize, profile: CompileProfile) -> usize {
    if profile.captures_ir_diff() {
        1
    } else {
        configured_jobs.max(1)
    }
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
            CompileProfile::O3Remarks.build_dir_name(),
            "build-tsvc-o3-remarks"
        );
        assert_eq!(
            CompileProfile::O3NoVec.build_dir_name(),
            "build-tsvc-o3-novec"
        );
        assert_eq!(
            CompileProfile::O3Default.build_dir_name(),
            "build-tsvc-o3-default"
        );
    }

    #[test]
    fn benchmark_path_contains_target_name() {
        let p = benchmark_binary_path(Path::new("/tmp/build"), "InductionVariable-dbl");
        assert!(p.ends_with("InductionVariable-dbl/InductionVariable-dbl"));
    }

    #[test]
    fn ir_capture_forces_single_job() {
        assert_eq!(effective_build_jobs(8, CompileProfile::O3Remarks), 1);
        assert_eq!(effective_build_jobs(8, CompileProfile::O3Default), 1);
    }

    #[test]
    fn render_command_includes_clean_first_for_build() {
        let mut build = Command::new("cmake");
        build
            .arg("--build")
            .arg("/tmp/build")
            .arg("--clean-first")
            .arg("--target")
            .arg("foo")
            .arg("-j")
            .arg("1");
        let rendered = render_command(&build);
        assert!(rendered.contains("--clean-first"));
    }
}
