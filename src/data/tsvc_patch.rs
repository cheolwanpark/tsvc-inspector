use std::fs;
use std::path::Path;

use anyhow::{Context, anyhow};
use regex::Regex;

use crate::core::error::AppResult;

const PATCH_MARKER: &str = "/* TSVC_TUI_FUNCTION_FILTER_PATCH */";
const HELPER_SNIPPET: &str = r#"/* TSVC_TUI_FUNCTION_FILTER_PATCH */
static int tsvc_tui_should_run(const char *loop_symbol) {
    const char *filter = getenv("TSVC_TUI_FUNCTION_FILTER");
    if (filter == NULL || filter[0] == '\0') {
        return 1;
    }
    return strcmp(filter, loop_symbol) == 0;
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsvcPatchOutcome {
    AlreadyPatched,
    Patched,
}

pub fn ensure_function_filter_patch(tsvc_root: &Path) -> AppResult<TsvcPatchOutcome> {
    let tsc_inc_path = tsvc_root
        .join("MultiSource")
        .join("Benchmarks")
        .join("TSVC")
        .join("tsc.inc");
    let original = fs::read_to_string(&tsc_inc_path)
        .with_context(|| format!("read {}", tsc_inc_path.display()))?;

    let Some(patched) = patch_tsc_inc_content(&original)? else {
        return Ok(TsvcPatchOutcome::AlreadyPatched);
    };

    let backup_path = tsc_inc_path.with_extension("inc.tsvc_tui_orig");
    if !backup_path.exists() {
        fs::write(&backup_path, &original)
            .with_context(|| format!("write {}", backup_path.display()))?;
    }

    fs::write(&tsc_inc_path, patched)
        .with_context(|| format!("write {}", tsc_inc_path.display()))?;
    Ok(TsvcPatchOutcome::Patched)
}

fn patch_tsc_inc_content(content: &str) -> AppResult<Option<String>> {
    if content.contains(PATCH_MARKER) {
        return Ok(None);
    }

    let wrapped = wrap_main_loop_calls(content)?;
    let Some(main_idx) = wrapped.find("int main(") else {
        return Err(anyhow!("failed to locate 'int main(' in tsc.inc"));
    };

    let mut out = String::new();
    out.push_str(&wrapped[..main_idx]);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(HELPER_SNIPPET);
    out.push('\n');
    out.push_str(&wrapped[main_idx..]);
    Ok(Some(out))
}

fn wrap_main_loop_calls(content: &str) -> AppResult<String> {
    let call_re = Regex::new(r"^(\s*)([A-Za-z_][A-Za-z0-9_]*)\s*\((.*)\);\s*$")?;

    let mut in_main = false;
    let mut in_call_block = false;
    let mut transformed = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim_start();

        if !in_main {
            if trimmed.starts_with("int main(") {
                in_main = true;
            }
            transformed.push(line.to_string());
            continue;
        }

        if !in_call_block && trimmed.contains("printf(\"Loop") {
            in_call_block = true;
            transformed.push(line.to_string());
            continue;
        }

        if in_call_block && trimmed.starts_with("return ") {
            transformed.push(line.to_string());
            in_call_block = false;
            continue;
        }

        if in_call_block
            && !trimmed.starts_with('#')
            && let Some(caps) = call_re.captures(line)
        {
            let indent = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let name = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
            let args = caps.get(3).map(|m| m.as_str()).unwrap_or_default();
            let wrapped_line =
                format!("{indent}if (tsvc_tui_should_run(\"{name}\")) {name}({args});");
            transformed.push(wrapped_line);
            continue;
        }

        transformed.push(line.to_string());
    }

    let mut output = transformed.join("\n");
    if content.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_injects_helper_and_wraps_loop_calls() {
        let input = r#"
int helper(void) { return 0; }

int main(int argc, char **argv) {
    printf("Loop \t Time(Sec) \t Checksum \n");
    s161();
    s162(n1);
#if TESTS & CONTROL_LOOPS
    va();
#endif
    return 0;
}
"#;

        let patched = patch_tsc_inc_content(input)
            .expect("patch should succeed")
            .expect("content should be patched");
        assert!(patched.contains(PATCH_MARKER));
        assert!(patched.contains("if (tsvc_tui_should_run(\"s161\")) s161();"));
        assert!(patched.contains("if (tsvc_tui_should_run(\"s162\")) s162(n1);"));
        assert!(patched.contains("if (tsvc_tui_should_run(\"va\")) va();"));
    }

    #[test]
    fn patch_is_idempotent() {
        let input = format!("{PATCH_MARKER}\nint main(int argc, char **argv) {{ return 0; }}\n");
        let patched = patch_tsc_inc_content(&input).expect("patch check");
        assert!(patched.is_none());
    }
}
