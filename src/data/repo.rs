use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow};

use crate::core::error::AppResult;
use crate::core::model::FunctionRunMode;
use crate::data::runner::RunnerConfig;
use crate::data::tsvc_patch::{self, TsvcPatchOutcome};

const LLVM_TEST_SUITE_REPO: &str = "https://github.com/llvm/llvm-test-suite.git";

pub fn resolve_tsvc_root(preferred_root: &Path) -> AppResult<PathBuf> {
    if has_tsvc_dir(preferred_root) {
        return Ok(preferred_root.to_path_buf());
    }

    let fallback = app_managed_fallback_root();
    if has_tsvc_dir(&fallback) {
        eprintln!(
            "TSVC root '{}' not found, using cached repository at '{}'",
            preferred_root.display(),
            fallback.display()
        );
        return Ok(fallback);
    }

    if fallback.exists() {
        fs::remove_dir_all(&fallback).with_context(|| {
            format!(
                "failed to remove incomplete fallback repository {}",
                fallback.display()
            )
        })?;
    }

    eprintln!(
        "TSVC root '{}' not found. Cloning llvm-test-suite to '{}'",
        preferred_root.display(),
        fallback.display()
    );
    clone_llvm_test_suite(&fallback)?;
    if !has_tsvc_dir(&fallback) {
        return Err(anyhow!(
            "cloned repository does not contain TSVC directory: {}",
            fallback.display()
        ));
    }

    Ok(fallback)
}

pub fn configure_function_run_mode(
    config: &RunnerConfig,
) -> AppResult<(FunctionRunMode, Option<String>)> {
    if !is_app_managed_fallback_root(&config.tsvc_root) {
        return Ok((
            FunctionRunMode::OutputFilter,
            Some(String::from(
                "External TSVC root detected: function mode = output-filter",
            )),
        ));
    }

    match tsvc_patch::ensure_function_filter_patch(&config.tsvc_root) {
        Ok(TsvcPatchOutcome::AlreadyPatched) => Ok((
            FunctionRunMode::RealSelective,
            Some(String::from("Function mode: real-selective")),
        )),
        Ok(TsvcPatchOutcome::Patched) => {
            if let Err(err) = reset_native_build_dirs(config) {
                eprintln!("warning: patched TSVC source but failed to reset build dirs: {err:#}");
            }
            Ok((
                FunctionRunMode::RealSelective,
                Some(String::from(
                    "Patched fallback TSVC for function-selective run mode",
                )),
            ))
        }
        Err(err) => {
            eprintln!("warning: failed to patch TSVC for real-selective mode: {err:#}");
            Ok((
                FunctionRunMode::OutputFilter,
                Some(String::from(
                    "Function patch failed: falling back to output-filter mode",
                )),
            ))
        }
    }
}

pub fn app_managed_fallback_root() -> PathBuf {
    std::env::temp_dir().join("tsvc-inspector-llvm-test-suite")
}

pub fn is_app_managed_fallback_root(root: &Path) -> bool {
    root == app_managed_fallback_root()
}

fn has_tsvc_dir(root: &Path) -> bool {
    root.join("MultiSource")
        .join("Benchmarks")
        .join("TSVC")
        .is_dir()
}

fn clone_llvm_test_suite(target_dir: &Path) -> AppResult<()> {
    if let Some(parent) = target_dir.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    run_checked(
        Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--filter=blob:none")
            .arg("--sparse")
            .arg(LLVM_TEST_SUITE_REPO)
            .arg(target_dir),
        "git clone llvm-test-suite",
    )?;

    run_checked(
        Command::new("git")
            .arg("-C")
            .arg(target_dir)
            .arg("sparse-checkout")
            .arg("set")
            .arg("MultiSource/Benchmarks/TSVC"),
        "git sparse-checkout set",
    )?;

    Ok(())
}

fn run_checked(command: &mut Command, label: &str) -> AppResult<()> {
    let output = command
        .output()
        .with_context(|| format!("failed to execute {label}"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(anyhow!(
        "{} failed with status {}.\nstdout:\n{}\nstderr:\n{}",
        label,
        output.status,
        stdout,
        stderr
    ))
}

fn reset_native_build_dirs(config: &RunnerConfig) -> AppResult<()> {
    let native_root = config.build_root.join("build-tsvc-native");
    if native_root.exists() {
        fs::remove_dir_all(&native_root)
            .with_context(|| format!("remove {}", native_root.display()))?;
    }

    for name in [
        "build-tsvc-o3-remarks-run",
        "build-tsvc-o3-novec-run",
        "build-tsvc-o3-default-run",
        "build-tsvc-o3-remarks-analysis",
        "build-tsvc-o3-novec-analysis",
        "build-tsvc-o3-default-analysis",
    ] {
        let dir = config.build_root.join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
        }
    }
    Ok(())
}
