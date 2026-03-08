use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use similar::{ChangeTag, TextDiff};

use crate::core::model::{
    AnalysisSource, AnalysisStage, AnalysisStep, DbgLocation, IrAttribute, IrAttributeOrigin,
    IrAttributeScope, IrLine, IrLineDetails, RemarkEntry,
};
use crate::data::parser::{normalize_pass_key, parse_dbg_locations, parse_ir_snapshots_from_trace};

const MAX_DIFF_LINES: usize = 8000;
type AttributeGroupMap = HashMap<u32, Vec<String>>;

pub fn build_fast_analysis_steps(
    build_trace: &str,
    selected_function_symbol: &str,
    remarks: &[RemarkEntry],
    source_file_content: Option<&str>,
) -> Vec<AnalysisStep> {
    let snapshots = parse_ir_snapshots_from_trace(build_trace);
    build_analysis_steps_from_snapshots(
        &snapshots,
        selected_function_symbol,
        remarks,
        AnalysisSource::TraceFast,
        source_file_content,
    )
}

pub fn annotate_ir_lines(
    ir_lines: Vec<IrLine>,
    dbg_after: &HashMap<u32, DbgLocation>,
    dbg_before: Option<&HashMap<u32, DbgLocation>>,
    source_lines: &[&str],
) -> (Vec<IrLine>, Vec<Option<u32>>) {
    static DBG_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"!dbg !(\d+)").expect("valid dbg ref regex"));
    static META_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:,\s*|\s+)![\w.]+\s+!\d+").expect("valid trailing metadata regex")
    });

    let mut out_lines = Vec::with_capacity(ir_lines.len());
    let mut out_map = Vec::with_capacity(ir_lines.len());
    let mut prev_src_key: Option<(u32, Option<String>, ChangeTag)> = None;

    for ir_line in ir_lines {
        if ir_line.text.trim_start().starts_with("#dbg_") {
            continue;
        }

        let dbg_map = if ir_line.tag == ChangeTag::Delete {
            dbg_before.unwrap_or(dbg_after)
        } else {
            dbg_after
        };

        let dbg_loc = DBG_RE.captures(&ir_line.text).and_then(|caps| {
            let id: u32 = caps.get(1)?.as_str().parse().ok()?;
            dbg_map.get(&id)
        });
        let src_line_no = dbg_loc.map(|loc| loc.line);

        if let Some(loc) = dbg_loc {
            let current_key = (loc.line, loc.inlined_from.clone(), ir_line.tag);
            let changed = prev_src_key.as_ref() != Some(&current_key);
            if changed {
                if let Some(src) = source_lines
                    .get((loc.line as usize).checked_sub(1).unwrap_or(usize::MAX))
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    let annotation = match &loc.inlined_from {
                        Some(name) => format!(";; [{name}] {src}"),
                        None => format!(";; {src}"),
                    };
                    out_lines.push(IrLine {
                        tag: ir_line.tag,
                        text: annotation,
                        is_source_annotation: true,
                        details: IrLineDetails::default(),
                    });
                    out_map.push(Some(loc.line));
                }
                prev_src_key = Some(current_key);
            }
        }

        let stripped = META_RE.replace_all(&ir_line.text, "");
        let stripped = stripped.trim_end();

        out_map.push(src_line_no);
        out_lines.push(IrLine {
            tag: ir_line.tag,
            text: stripped.to_string(),
            is_source_annotation: false,
            details: IrLineDetails::default(),
        });
    }

    (out_lines, out_map)
}

fn attach_line_attributes(
    ir_lines: &mut [IrLine],
    current_groups: &AttributeGroupMap,
    previous_groups: Option<&AttributeGroupMap>,
) {
    for ir_line in ir_lines {
        if ir_line.is_source_annotation {
            continue;
        }
        let groups = if ir_line.tag == ChangeTag::Delete {
            previous_groups.unwrap_or(current_groups)
        } else {
            current_groups
        };
        ir_line.details.attributes = parse_line_attributes(&ir_line.text, groups);
    }
}

fn parse_attribute_groups(snapshot: &str) -> AttributeGroupMap {
    static ATTR_GROUP_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"^\s*attributes #(\d+)\s*=\s*\{(.*)\}\s*$"#)
            .expect("valid attribute group regex")
    });

    let mut groups = HashMap::new();
    for line in snapshot.lines() {
        let Some(caps) = ATTR_GROUP_RE.captures(line) else {
            continue;
        };
        let Some(id) = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok()) else {
            continue;
        };
        let body = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
        groups.insert(id, split_attribute_tokens(body));
    }
    groups
}

fn parse_line_attributes(line: &str, groups: &AttributeGroupMap) -> Vec<IrAttribute> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed == "{" || trimmed == "}" || trimmed.ends_with(':') {
        return Vec::new();
    }

    if trimmed.starts_with("define ") || trimmed.starts_with("declare ") {
        return parse_function_line_attributes(trimmed, groups);
    }
    if find_call_keyword(trimmed).is_some() {
        return parse_call_line_attributes(trimmed, groups);
    }

    Vec::new()
}

fn parse_function_line_attributes(line: &str, groups: &AttributeGroupMap) -> Vec<IrAttribute> {
    let mut attrs = Vec::new();
    let Some(at_idx) = line.find('@') else {
        return attrs;
    };
    let Some(param_start_rel) = line[at_idx..].find('(') else {
        return attrs;
    };
    let param_start = at_idx + param_start_rel;
    let Some(param_end) = find_matching_delimiter(line, param_start, '(', ')') else {
        return attrs;
    };

    parse_zone_attributes(
        &line[..at_idx],
        IrAttributeScope::Return,
        groups,
        &mut attrs,
    );

    let params = &line[(param_start + 1)..param_end];
    for segment in split_top_level_commas(params) {
        parse_zone_attributes(segment, IrAttributeScope::Parameter, groups, &mut attrs);
    }

    let tail = truncate_attribute_tail(&line[(param_end + 1)..]);
    parse_zone_attributes(tail, IrAttributeScope::Function, groups, &mut attrs);
    attrs
}

fn parse_call_line_attributes(line: &str, groups: &AttributeGroupMap) -> Vec<IrAttribute> {
    let mut attrs = Vec::new();
    let Some((call_idx, keyword_len)) = find_call_keyword(line) else {
        return attrs;
    };
    let after_call = &line[(call_idx + keyword_len)..];
    let Some(args_start_rel) = after_call.find('(') else {
        return attrs;
    };
    let args_start = call_idx + keyword_len + args_start_rel;
    let Some(args_end) = find_matching_delimiter(line, args_start, '(', ')') else {
        return attrs;
    };

    parse_zone_attributes(
        &line[(call_idx + keyword_len)..args_start],
        IrAttributeScope::CallReturn,
        groups,
        &mut attrs,
    );

    let args = &line[(args_start + 1)..args_end];
    for segment in split_top_level_commas(args) {
        parse_zone_attributes(segment, IrAttributeScope::CallArgument, groups, &mut attrs);
    }

    let tail = truncate_attribute_tail(&line[(args_end + 1)..]);
    parse_zone_attributes(tail, IrAttributeScope::Call, groups, &mut attrs);
    attrs
}

fn parse_zone_attributes(
    text: &str,
    scope: IrAttributeScope,
    groups: &AttributeGroupMap,
    attrs: &mut Vec<IrAttribute>,
) {
    for token in split_attribute_tokens(text) {
        let token = trim_attribute_token(&token);
        if token.is_empty() || !looks_like_attribute_token(&token) {
            continue;
        }

        if let Some(group_id) = parse_group_ref(&token) {
            if let Some(group_attrs) = groups.get(&group_id) {
                for group_attr in group_attrs {
                    let group_attr = trim_attribute_token(group_attr);
                    if !group_attr.is_empty() && looks_like_attribute_token(&group_attr) {
                        push_attribute(
                            attrs,
                            scope,
                            group_attr,
                            IrAttributeOrigin::GroupRef(group_id),
                        );
                    }
                }
            }
            continue;
        }

        push_attribute(attrs, scope, token, IrAttributeOrigin::Inline);
    }
}

fn push_attribute(
    attrs: &mut Vec<IrAttribute>,
    scope: IrAttributeScope,
    text: String,
    origin: IrAttributeOrigin,
) {
    let candidate = IrAttribute {
        scope,
        text,
        origin,
    };
    if !attrs.iter().any(|existing| existing == &candidate) {
        attrs.push(candidate);
    }
}

fn find_call_keyword(line: &str) -> Option<(usize, usize)> {
    static CALL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b(call|invoke)\b").expect("valid call regex"));
    let caps = CALL_RE.captures(line)?;
    let m = caps.get(1)?;
    Some((m.start(), m.as_str().len()))
}

fn parse_group_ref(token: &str) -> Option<u32> {
    token
        .strip_prefix('#')
        .filter(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|rest| rest.parse::<u32>().ok())
}

fn trim_attribute_token(token: &str) -> String {
    token
        .trim()
        .trim_matches(|ch: char| ch == ',')
        .trim_matches(|ch: char| ch == ';')
        .to_string()
}

fn truncate_attribute_tail(text: &str) -> &str {
    let mut in_string = false;
    let mut paren_depth = 0i32;
    for (idx, ch) in text.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '(' if !in_string => paren_depth += 1,
            ')' if !in_string => paren_depth -= 1,
            '{' | ';' | '[' if !in_string && paren_depth == 0 => return &text[..idx],
            _ => {}
        }
    }
    text
}

fn split_attribute_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut paren_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut angle_depth = 0i32;

    for ch in text.chars() {
        match ch {
            '"' => {
                in_string = !in_string;
                current.push(ch);
            }
            '(' if !in_string => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' if !in_string => {
                paren_depth -= 1;
                current.push(ch);
            }
            '{' if !in_string => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' if !in_string => {
                brace_depth -= 1;
                current.push(ch);
            }
            '[' if !in_string => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' if !in_string => {
                bracket_depth -= 1;
                current.push(ch);
            }
            '<' if !in_string => {
                angle_depth += 1;
                current.push(ch);
            }
            '>' if !in_string && angle_depth > 0 => {
                angle_depth -= 1;
                current.push(ch);
            }
            ch if ch.is_whitespace()
                && !in_string
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }
    tokens
}

fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut in_string = false;
    let mut paren_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut angle_depth = 0i32;

    for (idx, ch) in text.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '(' if !in_string => paren_depth += 1,
            ')' if !in_string => paren_depth -= 1,
            '{' if !in_string => brace_depth += 1,
            '}' if !in_string => brace_depth -= 1,
            '[' if !in_string => bracket_depth += 1,
            ']' if !in_string => bracket_depth -= 1,
            '<' if !in_string => angle_depth += 1,
            '>' if !in_string && angle_depth > 0 => angle_depth -= 1,
            ',' if !in_string
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                segments.push(text[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }

    if start <= text.len() {
        segments.push(text[start..].trim());
    }
    segments
}

fn find_matching_delimiter(text: &str, start: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;

    for (idx, ch) in text.char_indices().skip_while(|(idx, _)| *idx < start) {
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(idx);
            }
        }
    }

    None
}

fn looks_like_attribute_token(token: &str) -> bool {
    if token.is_empty()
        || token == "..."
        || token.starts_with('@')
        || token.starts_with('%')
        || token.starts_with('!')
        || token == "="
        || token.ends_with(':')
        || is_non_attribute_keyword(token)
        || is_type_token(token)
    {
        return false;
    }

    if token.starts_with('"') {
        return true;
    }
    if token.starts_with('#') {
        return parse_group_ref(token).is_some();
    }
    if token.contains('=') || token.contains('(') {
        return true;
    }

    token
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic())
}

fn is_non_attribute_keyword(token: &str) -> bool {
    matches!(
        token,
        "define"
            | "declare"
            | "call"
            | "invoke"
            | "tail"
            | "musttail"
            | "notail"
            | "fastcc"
            | "coldcc"
            | "ccc"
            | "swiftcc"
            | "preserve_mostcc"
            | "preserve_allcc"
            | "x86_stdcallcc"
            | "x86_fastcallcc"
            | "x86_thiscallcc"
            | "spir_func"
            | "spir_kernel"
            | "dso_local"
            | "dso_preemptable"
            | "private"
            | "internal"
            | "available_externally"
            | "linkonce"
            | "weak"
            | "common"
            | "appending"
            | "extern_weak"
            | "linkonce_odr"
            | "weak_odr"
            | "external"
            | "unnamed_addr"
            | "local_unnamed_addr"
            | "addrspace"
            | "constant"
            | "global"
            | "alias"
            | "ifunc"
            | "to"
            | "within"
            | "blockaddress"
            | "asm"
            | "entry"
    )
}

fn is_type_token(token: &str) -> bool {
    if token.ends_with('*')
        || token.starts_with("ptr")
        || token
            .strip_prefix('i')
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
        || matches!(
            token,
            "void"
                | "half"
                | "bfloat"
                | "float"
                | "double"
                | "fp128"
                | "x86_fp80"
                | "ppc_fp128"
                | "label"
                | "token"
                | "metadata"
                | "x86_amx"
        )
        || token.starts_with('{')
        || token.starts_with('[')
        || token.starts_with('<')
    {
        return true;
    }

    false
}

fn build_analysis_steps_from_snapshots(
    snapshots: &[crate::data::parser::IrSnapshot],
    selected_function_symbol: &str,
    remarks: &[RemarkEntry],
    source: AnalysisSource,
    source_file_content: Option<&str>,
) -> Vec<AnalysisStep> {
    let src_lines: Vec<&str> = source_file_content
        .map(|s| s.lines().collect())
        .unwrap_or_default();

    let mut out = Vec::new();
    let mut prev_ir: Option<String> = None;
    let mut prev_dbg_locations: Option<HashMap<u32, DbgLocation>> = None;
    let mut prev_attribute_groups: Option<AttributeGroupMap> = None;
    let mut prev_raw_index = 0usize;

    for snapshot in snapshots {
        let Some(function_ir) = extract_function_ir(&snapshot.snapshot, selected_function_symbol)
        else {
            continue;
        };

        let target_function = target_function_from_label(&snapshot.target);
        let stage = classify_analysis_stage(snapshot.raw_index, &snapshot.pass, &snapshot.target);
        let remark_indices =
            collect_analysis_remark_indices(&snapshot.pass, target_function.as_deref(), remarks);

        let dbg_locations = parse_dbg_locations(&snapshot.snapshot);
        let attribute_groups = parse_attribute_groups(&snapshot.snapshot);

        if prev_ir.is_none() {
            let ir_lines: Vec<IrLine> = function_ir
                .lines()
                .map(|l| IrLine {
                    tag: ChangeTag::Equal,
                    text: l.to_string(),
                    is_source_annotation: false,
                    details: IrLineDetails::default(),
                })
                .collect();
            let (mut ir_lines, source_line_map) =
                annotate_ir_lines(ir_lines, &dbg_locations, None, &src_lines);
            attach_line_attributes(&mut ir_lines, &attribute_groups, None);
            out.push(AnalysisStep {
                visible_index: out.len(),
                raw_index: snapshot.raw_index,
                pass: snapshot.pass.clone(),
                pass_key: normalize_pass_key(&snapshot.pass),
                pass_occurrence: snapshot.pass_occurrence.max(1),
                stage,
                target_raw: snapshot.target.clone(),
                target_function,
                changed_lines: 0,
                diff_text: String::from("No previous step. This is the initial IR snapshot."),
                ir_lines,
                source_line_map,
                remark_indices,
                source,
            });
            prev_raw_index = snapshot.raw_index;
            prev_dbg_locations = Some(dbg_locations);
            prev_attribute_groups = Some(attribute_groups);
            prev_ir = Some(function_ir);
            continue;
        }

        let Some(before) = prev_ir.as_ref() else {
            continue;
        };
        if before == &function_ir {
            continue;
        }

        let diff = TextDiff::from_lines(before, &function_ir);
        let changed_lines = diff
            .iter_all_changes()
            .filter(|change| change.tag() != ChangeTag::Equal)
            .count();

        let ir_lines: Vec<IrLine> = diff
            .iter_all_changes()
            .map(|change| IrLine {
                tag: change.tag(),
                text: change.value().trim_end_matches('\n').to_string(),
                is_source_annotation: false,
                details: IrLineDetails::default(),
            })
            .collect();
        let (mut ir_lines, source_line_map) = annotate_ir_lines(
            ir_lines,
            &dbg_locations,
            prev_dbg_locations.as_ref(),
            &src_lines,
        );
        attach_line_attributes(
            &mut ir_lines,
            &attribute_groups,
            prev_attribute_groups.as_ref(),
        );

        let mut diff_text = diff
            .unified_diff()
            .context_radius(3)
            .header(
                &format!("raw-{prev_raw_index:05}"),
                &format!("raw-{:05}", snapshot.raw_index),
            )
            .to_string();
        if diff_text.trim().is_empty() {
            diff_text = String::from("No textual diff was produced for this step.");
        } else {
            diff_text = truncate_lines(&diff_text, MAX_DIFF_LINES);
        }

        out.push(AnalysisStep {
            visible_index: out.len(),
            raw_index: snapshot.raw_index,
            pass: snapshot.pass.clone(),
            pass_key: normalize_pass_key(&snapshot.pass),
            pass_occurrence: snapshot.pass_occurrence.max(1),
            stage,
            target_raw: snapshot.target.clone(),
            target_function,
            changed_lines,
            diff_text,
            ir_lines,
            source_line_map,
            remark_indices,
            source,
        });

        prev_raw_index = snapshot.raw_index;
        prev_dbg_locations = Some(dbg_locations);
        prev_attribute_groups = Some(attribute_groups);
        prev_ir = Some(function_ir);
    }

    out
}

fn collect_analysis_remark_indices(
    pass: &str,
    target_function: Option<&str>,
    remarks: &[RemarkEntry],
) -> Vec<usize> {
    let pass_key = normalize_pass_key(pass);
    let matching = remarks
        .iter()
        .enumerate()
        .filter(|(_, remark)| normalize_pass_key(&remark.pass) == pass_key)
        .collect::<Vec<_>>();

    let Some(target_function) = target_function else {
        return matching.into_iter().map(|(idx, _)| idx).collect();
    };

    let exact = matching
        .iter()
        .filter_map(|(idx, remark)| {
            if remark
                .function
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case(target_function))
            {
                Some(*idx)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if !exact.is_empty() {
        return exact;
    }

    matching.into_iter().map(|(idx, _)| idx).collect()
}

fn classify_analysis_stage(raw_index: usize, pass: &str, target: &str) -> AnalysisStage {
    if raw_index == 0 {
        return AnalysisStage::Initial;
    }

    let pass_key = normalize_pass_key(pass);
    let target_lower = target.to_ascii_lowercase();

    let vector_keywords = [
        "loopvectorize",
        "slpvectorizer",
        "vectorcombine",
        "looploadelimination",
        "looploadelim",
        "inferalignment",
        "injecttlimappings",
        "vectorize",
    ];
    if pass_contains_any(&pass_key, &vector_keywords) {
        return AnalysisStage::Vectorize;
    }

    let loop_keywords = [
        "loop",
        "lcssa",
        "licm",
        "indvar",
        "unswitch",
        "loopsink",
        "loopunroll",
        "loopsimplify",
        "loopdeletion",
        "loopdistribute",
        "loopinterchange",
        "loopidiom",
        "loopflatten",
        "loopfuse",
        "loopinstsimplify",
        "loopsimplifycfg",
        "loopversioning",
    ];
    if target_lower.starts_with("loop ") || pass_contains_any(&pass_key, &loop_keywords) {
        return AnalysisStage::Loop;
    }

    let interprocedural_keywords = [
        "inline",
        "ipsccp",
        "openmpopt",
        "calledvaluepropagation",
        "globalopt",
        "globaldce",
        "deadargument",
        "functionattrs",
        "attributor",
        "cgprofile",
        "constmerge",
        "elimavailextern",
        "recomputeglobalsaa",
        "rpofunctionattrs",
        "rellookuptableconverter",
        "annotation2metadata",
        "forceattrs",
        "inferattrs",
        "memprofremoveattributes",
        "devirt",
        "moduleinliner",
        "partialinliner",
    ];
    let is_interproc_target =
        target == "[module]" || (target.starts_with('(') && target.ends_with(')'));
    if is_interproc_target || pass_contains_any(&pass_key, &interprocedural_keywords) {
        return AnalysisStage::Interprocedural;
    }

    if pass_key.is_empty() {
        return AnalysisStage::Other;
    }

    AnalysisStage::Cleanup
}

fn pass_contains_any(pass_key: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| pass_key.contains(keyword))
}

fn target_function_from_label(target: &str) -> Option<String> {
    let trimmed = target.trim();
    if trimmed.is_empty() || trimmed == "[module]" {
        return None;
    }
    if let Some(inner) = trimmed
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        && looks_like_symbol(inner)
    {
        return Some(inner.to_string());
    }
    if let Some((_, function_name)) = trimmed.rsplit_once(" in function ")
        && looks_like_symbol(function_name)
    {
        return Some(function_name.to_string());
    }
    if looks_like_symbol(trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

fn looks_like_symbol(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
}

fn extract_function_ir(snapshot: &str, selected_symbol: &str) -> Option<String> {
    let lines = snapshot.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    for (start, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("define ") {
            continue;
        }
        let Some(name) = parse_define_symbol(trimmed) else {
            continue;
        };
        if !name.eq_ignore_ascii_case(selected_symbol) {
            continue;
        }

        let mut end = start;
        while end < lines.len() && lines[end].trim() != "}" {
            end += 1;
        }
        if end >= lines.len() {
            end = lines.len() - 1;
        }
        return Some(lines[start..=end].join("\n"));
    }

    None
}

fn parse_define_symbol(define_line: &str) -> Option<String> {
    let at_idx = define_line.find('@')?;
    let symbol_with_rest = &define_line[(at_idx + 1)..];
    if symbol_with_rest.is_empty() {
        return None;
    }

    let mut name = String::new();
    for ch in symbol_with_rest.chars() {
        if ch == '(' {
            break;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            name.push(ch);
        } else if name.is_empty() {
            continue;
        } else {
            break;
        }
    }

    if name.is_empty() { None } else { Some(name) }
}

fn truncate_lines(text: &str, max_lines: usize) -> String {
    let mut lines = text.lines();
    let kept = lines.by_ref().take(max_lines).collect::<Vec<_>>();
    let dropped = lines.count();
    if dropped == 0 {
        return text.to_string();
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("... [truncated {dropped} lines]"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::RemarkKind;

    #[test]
    fn fast_analysis_builds_changed_only_function_timeline() {
        let trace = r#"
*** IR Dump At Start ***
define i32 @s161() !dbg !14 {
entry:
  %0 = add i32 1, 2, !dbg !9
  ret i32 %0, !dbg !10
}
!9 = !DILocation(line: 10, column: 3, scope: !14)
!10 = !DILocation(line: 11, column: 3, scope: !14)
!14 = distinct !DISubprogram(name: "s161", scope: !1, file: !1, line: 1)
*** IR Dump After LoopVectorizePass on s161 ***
define i32 @s161() !dbg !14 {
entry:
  %0 = add i32 1, 4, !dbg !9
  ret i32 %0, !dbg !10
}
!9 = !DILocation(line: 10, column: 3, scope: !14)
!10 = !DILocation(line: 11, column: 3, scope: !14)
!14 = distinct !DISubprogram(name: "s161", scope: !1, file: !1, line: 1)
"#;

        let remarks = vec![RemarkEntry {
            kind: RemarkKind::Passed,
            pass: "loop-vectorize".to_string(),
            name: "Vectorized".to_string(),
            file: None,
            line: None,
            function: Some("s161".to_string()),
            message: Some("vectorized loop".to_string()),
        }];

        let steps = build_fast_analysis_steps(trace, "s161", &remarks, Some("line10\nline11\n"));
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].stage, AnalysisStage::Initial);
        assert_eq!(steps[1].stage, AnalysisStage::Vectorize);
        assert!(steps[1].changed_lines > 0);
    }

    #[test]
    fn initial_ir_lines_capture_inline_and_group_attributes() {
        let trace = r#"
*** IR Dump At Start ***
define noundef i32 @s161(ptr noalias noundef %p) #0 !dbg !14 {
entry:
  %0 = call noundef i32 @foo(ptr nonnull %p) nounwind
  ret i32 %0, !dbg !10
}
attributes #0 = { alwaysinline "no-sse" }
!10 = !DILocation(line: 11, column: 3, scope: !14)
!14 = distinct !DISubprogram(name: "s161", scope: !1, file: !1, line: 1)
"#;

        let steps = build_fast_analysis_steps(trace, "s161", &[], Some("line10\nline11\n"));
        let define_line = steps[0]
            .ir_lines
            .iter()
            .find(|line| line.text.starts_with("define "))
            .expect("define line should exist");
        let call_line = steps[0]
            .ir_lines
            .iter()
            .find(|line| line.text.contains("call"))
            .expect("call line should exist");

        let define_attrs = define_line
            .details
            .attributes
            .iter()
            .map(|attr| attr.text.as_str())
            .collect::<Vec<_>>();
        assert!(define_attrs.contains(&"noundef"));
        assert!(define_attrs.contains(&"noalias"));
        assert!(define_attrs.contains(&"alwaysinline"));
        assert!(define_attrs.contains(&"\"no-sse\""));
        assert!(define_line.details.attributes.iter().any(|attr| {
            attr.text == "alwaysinline" && matches!(attr.origin, IrAttributeOrigin::GroupRef(0))
        }));

        let call_attrs = call_line
            .details
            .attributes
            .iter()
            .map(|attr| attr.text.as_str())
            .collect::<Vec<_>>();
        assert!(call_attrs.contains(&"noundef"));
        assert!(call_attrs.contains(&"nonnull"));
        assert!(call_attrs.contains(&"nounwind"));
    }

    #[test]
    fn diff_lines_use_correct_group_table_for_deleted_and_inserted_lines() {
        let trace = r#"
*** IR Dump At Start ***
define i32 @s161() #0 !dbg !14 {
entry:
  ret i32 0, !dbg !10
}
attributes #0 = { nounwind }
!10 = !DILocation(line: 11, column: 3, scope: !14)
!14 = distinct !DISubprogram(name: "s161", scope: !1, file: !1, line: 1)
*** IR Dump After ForceFunctionAttrsPass on s161 ***
define i32 @s161() #1 !dbg !14 {
entry:
  ret i32 0, !dbg !10
}
attributes #1 = { readonly }
!10 = !DILocation(line: 11, column: 3, scope: !14)
!14 = distinct !DISubprogram(name: "s161", scope: !1, file: !1, line: 1)
"#;

        let steps = build_fast_analysis_steps(trace, "s161", &[], Some("line10\nline11\n"));
        let diff_step = &steps[1];
        let deleted_define = diff_step
            .ir_lines
            .iter()
            .find(|line| line.tag == ChangeTag::Delete && line.text.starts_with("define "))
            .expect("deleted define line should exist");
        let inserted_define = diff_step
            .ir_lines
            .iter()
            .find(|line| line.tag == ChangeTag::Insert && line.text.starts_with("define "))
            .expect("inserted define line should exist");

        assert!(
            deleted_define
                .details
                .attributes
                .iter()
                .any(|attr| attr.text == "nounwind")
        );
        assert!(
            inserted_define
                .details
                .attributes
                .iter()
                .any(|attr| attr.text == "readonly")
        );
    }
}
