use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow};

use crate::error::AppResult;

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

pub fn app_managed_fallback_root() -> PathBuf {
    std::env::temp_dir().join("tsvc-tui-llvm-test-suite")
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
            .arg("MultiSource/Benchmarks/TSVC")
            .arg("cmake")
            .arg("litsupport")
            .arg("tools"),
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
