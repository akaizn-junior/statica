//! Byte-offset → line/column helpers for authoring diagnostics.

use std::fmt;

/// Authoring diagnostic: `file:line:column: message` + optional rustc-style snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
    pub snippet: String,
}

impl Diagnostic {
    /// Locate via needle search in `source`, or fall back to `file:1:1` with no snippet.
    #[must_use]
    pub fn at(file: &str, source: &str, needles: &[&str], message: impl Into<String>) -> Self {
        let (file, line, column, snippet) = locate_any(file, source, needles);
        Self {
            file,
            line,
            column,
            message: message.into(),
            snippet,
        }
    }

    /// Path known, no useful source span (still emits `file:1:1:`).
    #[must_use]
    pub fn at_file(file: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            line: 1,
            column: 1,
            message: message.into(),
            snippet: String::new(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.file, self.line, self.column, self.message
        )?;
        if !self.snippet.is_empty() {
            write!(f, "\n{}", self.snippet)?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostic {}

/// 1-based line and column for a byte offset into `source`.
#[must_use]
pub fn offset_to_line_col(source: &str, byte_offset: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, ch) in source.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Text of `line` (1-based), without the trailing newline.
#[must_use]
pub fn line_text(source: &str, line: u32) -> String {
    source
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .unwrap_or("")
        .to_string()
}

/// Find `needle` in `source` and return `(offset, line, column)`.
#[must_use]
pub fn find_pos(source: &str, needle: &str) -> Option<(usize, u32, u32)> {
    let offset = source.find(needle)?;
    let (line, column) = offset_to_line_col(source, offset);
    Some((offset, line, column))
}

/// First match among needles.
#[must_use]
pub fn find_pos_any<'a>(source: &str, needles: &[&'a str]) -> Option<(usize, u32, u32, &'a str)> {
    let mut best: Option<(usize, u32, u32, &str)> = None;
    for needle in needles {
        if needle.is_empty() {
            continue;
        }
        if let Some((offset, line, column)) = find_pos(source, needle) {
            if best.map_or(true, |(o, _, _, _)| offset < o) {
                best = Some((offset, line, column, needle));
            }
        }
    }
    best
}

/// Rustc-style snippet under a span: ` LINE | text` + caret row.
#[must_use]
pub fn snippet(source: &str, line: u32, column: u32, highlight_len: usize) -> String {
    let text = line_text(source, line);
    let gutter = line.to_string();
    let pad = " ".repeat(gutter.len());
    let caret_col = (column as usize).saturating_sub(1).min(text.len());
    let len = highlight_len.max(1).min(text.len().saturating_sub(caret_col).max(1));
    format!(
        "{pad} |\n{gutter} | {text}\n{pad} | {}{}",
        " ".repeat(caret_col),
        "^".repeat(len)
    )
}

/// Location + snippet for the earliest needle match, or `1:1` with empty snippet.
#[must_use]
pub fn locate_any(file: &str, source: &str, needles: &[&str]) -> (String, u32, u32, String) {
    if let Some((_offset, line, column, needle)) = find_pos_any(source, needles) {
        let snip = snippet(source, line, column, needle.chars().count());
        (file.to_string(), line, column, snip)
    } else {
        (file.to_string(), 1, 1, String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_cols() {
        let src = "a\nbc${x}\nd";
        let (_, line, col) = find_pos(src, "${x}").unwrap();
        assert_eq!((line, col), (2, 3));
        let snip = snippet(src, line, col, 4);
        assert!(snip.contains("2 | bc${x}"));
        assert!(snip.contains("^^^"));
    }

    #[test]
    fn diagnostic_display() {
        let d = Diagnostic::at("ui/x.html", "<a href=\"${href}\">", &["${href}"], "not bound");
        let s = d.to_string();
        assert!(s.starts_with("ui/x.html:1:"));
        assert!(s.contains("not bound"));
        assert!(s.contains("${href}"));
    }
}
