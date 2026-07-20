//! Byte-offset → line/column helpers for authoring diagnostics.

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
}
