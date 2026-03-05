use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use ratatui::style::{Color, Modifier, Style};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

const CACHE_CAPACITY: usize = 32;

const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "escape",
    "function",
    "function.builtin",
    "identifier",
    "keyword",
    "label",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyntaxLang {
    C,
    LlvmIr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledChunk {
    pub text: String,
    pub style: Style,
}

pub type HighlightedLines = Arc<Vec<Vec<StyledChunk>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    lang: SyntaxLang,
    hash: u64,
    len: usize,
}

impl CacheKey {
    fn new(lang: SyntaxLang, source: &str) -> Self {
        Self {
            lang,
            hash: hash_text(source),
            len: source.len(),
        }
    }
}

struct HighlightCache {
    entries: HashMap<CacheKey, HighlightedLines>,
    order: VecDeque<CacheKey>,
    capacity: usize,
}

impl HighlightCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn get(&mut self, key: &CacheKey) -> Option<HighlightedLines> {
        let value = self.entries.get(key).cloned()?;
        self.touch(*key);
        Some(value)
    }

    fn insert(&mut self, key: CacheKey, value: HighlightedLines) {
        if self.capacity == 0 {
            return;
        }

        self.entries.insert(key, value);
        self.touch(key);

        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }

    fn touch(&mut self, key: CacheKey) {
        if let Some(pos) = self.order.iter().position(|k| *k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key);
    }
}

struct SyntaxEngine {
    c_config: HighlightConfiguration,
    llvm_config: HighlightConfiguration,
    capture_styles: Vec<Style>,
    highlighter: Highlighter,
    cache: HighlightCache,
}

impl SyntaxEngine {
    fn new() -> Result<Self, String> {
        let mut c_config = HighlightConfiguration::new(
            tree_sitter_c::LANGUAGE.into(),
            "c",
            tree_sitter_c::HIGHLIGHT_QUERY,
            "",
            "",
        )
        .map_err(|e| format!("failed to initialize C highlight configuration: {e}"))?;

        let mut llvm_config = HighlightConfiguration::new(
            tree_sitter_llvm::LANGUAGE.into(),
            "llvm",
            tree_sitter_llvm::HIGHLIGHTS_QUERY,
            "",
            "",
        )
        .map_err(|e| format!("failed to initialize LLVM IR highlight configuration: {e}"))?;

        c_config.configure(HIGHLIGHT_NAMES);
        llvm_config.configure(HIGHLIGHT_NAMES);

        let capture_styles = HIGHLIGHT_NAMES
            .iter()
            .map(|name| style_for_capture_name(name))
            .collect();

        Ok(Self {
            c_config,
            llvm_config,
            capture_styles,
            highlighter: Highlighter::new(),
            cache: HighlightCache::new(CACHE_CAPACITY),
        })
    }

    fn highlight_cached(&mut self, lang: SyntaxLang, source: &str) -> HighlightedLines {
        let key = CacheKey::new(lang, source);
        if let Some(cached) = self.cache.get(&key) {
            return cached;
        }

        let computed = self
            .highlight_uncached(lang, source)
            .unwrap_or_else(|| plain_highlighted_lines(source));
        self.cache.insert(key, Arc::clone(&computed));
        computed
    }

    fn highlight_uncached(&mut self, lang: SyntaxLang, source: &str) -> Option<HighlightedLines> {
        let config = match lang {
            SyntaxLang::C => &self.c_config,
            SyntaxLang::LlvmIr => &self.llvm_config,
        };

        let bytes = source.as_bytes();
        let events = self
            .highlighter
            .highlight(config, bytes, None, |_| None)
            .ok()?;

        let mut active_styles = Vec::<Style>::new();
        let mut current_style = Style::default();
        let mut lines: Vec<Vec<StyledChunk>> = vec![Vec::new()];

        for event in events {
            match event.ok()? {
                HighlightEvent::Source { start, end } => {
                    let segment = String::from_utf8_lossy(&bytes[start..end]);
                    append_text(&mut lines, &segment, current_style);
                }
                HighlightEvent::HighlightStart(highlight) => {
                    let style = self
                        .capture_styles
                        .get(highlight.0)
                        .copied()
                        .unwrap_or_default();
                    active_styles.push(style);
                    current_style = compose_styles(&active_styles);
                }
                HighlightEvent::HighlightEnd => {
                    active_styles.pop();
                    current_style = compose_styles(&active_styles);
                }
            }
        }

        normalize_lines(source, &mut lines);
        Some(Arc::new(lines))
    }
}

pub fn highlight(lang: SyntaxLang, source: &str) -> HighlightedLines {
    let Some(mutex) = engine() else {
        return plain_highlighted_lines(source);
    };

    match mutex.lock() {
        Ok(mut engine) => engine.highlight_cached(lang, source),
        Err(_) => plain_highlighted_lines(source),
    }
}

fn engine() -> Option<&'static Mutex<SyntaxEngine>> {
    static ENGINE: OnceLock<Result<Mutex<SyntaxEngine>, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| SyntaxEngine::new().map(Mutex::new))
        .as_ref()
        .ok()
}

fn plain_highlighted_lines(source: &str) -> HighlightedLines {
    let mut lines = vec![Vec::new()];
    append_text(&mut lines, source, Style::default());
    normalize_lines(source, &mut lines);
    Arc::new(lines)
}

fn append_text(lines: &mut Vec<Vec<StyledChunk>>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }

    for chunk in text.split_inclusive('\n') {
        let ends_with_newline = chunk.ends_with('\n');
        let content = if ends_with_newline {
            &chunk[..chunk.len() - 1]
        } else {
            chunk
        };

        if !content.is_empty() {
            push_chunk(lines.last_mut().expect("at least one line"), content, style);
        }

        if ends_with_newline {
            lines.push(Vec::new());
        }
    }
}

fn push_chunk(line: &mut Vec<StyledChunk>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }

    if let Some(last) = line.last_mut()
        && last.style == style
    {
        last.text.push_str(text);
        return;
    }

    line.push(StyledChunk {
        text: text.to_string(),
        style,
    });
}

fn normalize_lines(source: &str, lines: &mut Vec<Vec<StyledChunk>>) {
    if source.ends_with('\n') && lines.last().is_some_and(Vec::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
}

fn compose_styles(styles: &[Style]) -> Style {
    styles
        .iter()
        .copied()
        .fold(Style::default(), |acc, s| acc.patch(s))
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn style_for_capture_name(name: &str) -> Style {
    let base = name.split('.').next().unwrap_or(name);
    match base {
        "comment" => Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::ITALIC),
        "string" | "escape" => Style::default().fg(Color::LightGreen),
        "number" | "constant" => Style::default().fg(Color::LightMagenta),
        "keyword" => Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD),
        "type" => Style::default().fg(Color::LightBlue),
        "function" | "constructor" => Style::default().fg(Color::LightCyan),
        "attribute" | "property" | "label" | "tag" => Style::default().fg(Color::Cyan),
        "operator" => Style::default().fg(Color::White),
        "punctuation" => Style::default().fg(Color::Gray),
        _ => Style::default(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{SyntaxEngine, SyntaxLang, highlight, plain_highlighted_lines};

    #[test]
    fn c_highlight_preserves_line_count() {
        let highlighted = highlight(SyntaxLang::C, "int x = 1;\nreturn x;\n");
        assert_eq!(highlighted.len(), 2);
    }

    #[test]
    fn llvm_highlight_preserves_line_count() {
        let highlighted = highlight(
            SyntaxLang::LlvmIr,
            "define void @f() {\nentry:\n  ret void\n}\n",
        );
        assert_eq!(highlighted.len(), 4);
    }

    #[test]
    fn cache_hit_reuses_arc_for_same_input() {
        let mut engine = SyntaxEngine::new().expect("syntax engine should initialize");
        let first = engine.highlight_cached(SyntaxLang::C, "int x = 1;\n");
        let second = engine.highlight_cached(SyntaxLang::C, "int x = 1;\n");
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn plain_fallback_normalizes_trailing_newline() {
        let lines = plain_highlighted_lines("a\nb\n");
        assert_eq!(lines.len(), 2);
    }
}
