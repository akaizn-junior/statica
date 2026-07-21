//! Pre-html5ever authoring normalization.
//!
//! Minor source transforms so html5ever preserves Statica-specific syntax that
//! browsers reject (e.g. `<slot>` inside `<select>`).

use crate::error::{Error, Result};

/// Normalize authoring HTML before html5ever parsing.
pub fn preprocess(input: &str) -> Result<String> {
    let lower = input.to_ascii_lowercase();
    if !lower.contains("<slot") {
        return Ok(input.to_string());
    }

    let mut out = String::with_capacity(input.len());
    let mut pos = 0usize;

    while let Some(rel) = find_tag_open(&lower, pos, "select") {
        let start = pos + rel;
        out.push_str(&input[pos..start]);

        let open_end = find_gt(input, start)?;
        let inner_end = find_closing_tag(&lower, open_end, "select")?;

        out.push_str(&input[start..open_end]);
        out.push_str(&rewrite_select_inner(
            &input[open_end..inner_end],
            &lower[open_end..inner_end],
        )?);
        out.push_str("</select>");

        pos = inner_end + "</select>".len();
    }

    out.push_str(&input[pos..]);
    Ok(out)
}

fn rewrite_select_inner(inner: &str, inner_lower: &str) -> Result<String> {
    let mut out = String::new();
    let mut pos = 0usize;

    while pos < inner.len() {
        if inner_lower.as_bytes().get(pos) != Some(&b'<') {
            out.push(inner.as_bytes()[pos] as char);
            pos += 1;
            continue;
        }

        if find_tag_open(&inner_lower[pos..], 0, "slot") == Some(0) {
            let start = pos;
            let end = find_element_end(inner, inner_lower, start, "slot")?;
            out.push_str(&slot_to_script(&inner[start..end])?);
            pos = end;
            continue;
        }

        if find_tag_open(&inner_lower[pos..], 0, "optgroup") == Some(0) {
            let start = pos;
            let open_end = find_gt(inner, start)?;
            let inner_end = find_closing_tag(inner_lower, open_end, "optgroup")?;
            out.push_str(&inner[start..open_end]);
            out.push_str(&rewrite_select_inner(
                &inner[open_end..inner_end],
                &inner_lower[open_end..inner_end],
            )?);
            out.push_str("</optgroup>");
            pos = inner_end + "</optgroup>".len();
            continue;
        }

        if let Some(tag) = tag_name_at(inner_lower, pos) {
            let end = find_element_end(inner, inner_lower, pos, &tag)?;
            out.push_str(&inner[pos..end]);
            pos = end;
            continue;
        }

        out.push(inner.as_bytes()[pos] as char);
        pos += 1;
    }

    Ok(out)
}

/// `<slot …>` → `<script type="statica/slot" …>` (valid inside `<select>`).
fn slot_to_script(slot_html: &str) -> Result<String> {
    let lower = slot_html.to_ascii_lowercase();
    if find_tag_open(&lower, 0, "slot") != Some(0) {
        return Err(Error::msg("expected <slot> element"));
    }

    let open_end = find_gt(slot_html, 0)?;
    let mut attrs = slot_html[..open_end]
        .split_at(
            slot_html[..open_end]
                .find('<')
                .map(|i| i + 1)
                .unwrap_or(0),
        )
        .1
        .trim();
    if let Some(stripped) = attrs.strip_prefix("slot") {
        attrs = stripped.trim();
    } else if let Some(stripped) = attrs.strip_prefix("SLOT") {
        attrs = stripped.trim();
    }
    if attrs.ends_with('/') {
        attrs = attrs.trim_end_matches('/').trim();
    }

    let close = lower.rfind("</slot");
    let inner = match close {
        Some(idx) if idx >= open_end => &slot_html[open_end..idx],
        _ => "",
    };

    Ok(if attrs.is_empty() {
        format!("<script type=\"statica/slot\">{inner}</script>")
    } else {
        format!("<script type=\"statica/slot\" {attrs}>{inner}</script>")
    })
}

fn tag_name_at(lower: &str, pos: usize) -> Option<String> {
    if lower.as_bytes().get(pos) != Some(&b'<') {
        return None;
    }
    let rest = &lower[pos + 1..];
    if rest.starts_with('/') {
        return None;
    }
    let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
    if end == 0 {
        return None;
    }
    Some(rest[..end].to_string())
}

fn find_tag_open(lower: &str, from: usize, tag: &str) -> Option<usize> {
    let needle = format!("<{tag}");
    let mut i = from;
    while i < lower.len() {
        if let Some(rel) = lower[i..].find(&needle) {
            let at = i + rel;
            if tag_boundary(lower, at, tag) {
                return Some(at - from);
            }
            i = at + 1;
        } else {
            break;
        }
    }
    None
}

fn tag_boundary(lower: &str, at: usize, tag: &str) -> bool {
    let after = at + tag.len() + 1;
    after >= lower.len()
        || matches!(
            lower.as_bytes()[after],
            b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>'
        )
}

fn find_gt(raw: &str, from: usize) -> Result<usize> {
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in raw[from..].char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '>' if !in_single && !in_double => return Ok(from + i + 1),
            _ => {}
        }
    }
    Err(Error::msg("unclosed tag"))
}

fn find_closing_tag(lower: &str, content_start: usize, tag: &str) -> Result<usize> {
    let close = format!("</{tag}>");
    let mut depth = 1usize;
    let mut pos = content_start;
    while pos < lower.len() {
        if lower[pos..].starts_with(&close) {
            depth -= 1;
            if depth == 0 {
                return Ok(pos);
            }
            pos += close.len();
            continue;
        }
        if lower[pos..].starts_with('<') && find_tag_open(&lower[pos..], 0, tag).is_some() {
            depth += 1;
            pos += 1;
            continue;
        }
        pos += 1;
    }
    Err(Error::msg(format!("unclosed <{tag}>")))
}

fn find_element_end(raw: &str, lower: &str, start: usize, tag: &str) -> Result<usize> {
    let open_end = find_gt(raw, start)?;
    if raw[start..open_end].trim_end().ends_with('/') {
        return Ok(open_end);
    }
    let close = format!("</{tag}>");
    let mut depth = 1usize;
    let mut pos = open_end;
    while pos < lower.len() {
        if lower[pos..].starts_with(&close) {
            depth -= 1;
            if depth == 0 {
                return Ok(pos + close.len());
            }
            pos += close.len();
            continue;
        }
        if find_tag_open(&lower[pos..], 0, tag) == Some(0) {
            depth += 1;
        }
        pos += 1;
    }
    Err(Error::msg(format!("unclosed <{tag}>")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_slot_inside_select() {
        let raw = r#"<select name="c"><slot id="a" data-each="items"></slot></select>"#;
        let out = preprocess(raw).unwrap();
        assert!(out.contains(r#"type="statica/slot""#));
        assert!(out.contains(r#"id="a""#));
        assert!(!out.contains("<slot"));
    }

    #[test]
    fn rewrites_slot_inside_optgroup() {
        let raw =
            r#"<select><optgroup label="G"><slot id="b" data-each="items"></slot></optgroup></select>"#;
        let out = preprocess(raw).unwrap();
        assert!(out.contains(r#"type="statica/slot""#));
        assert!(out.contains(r#"id="b""#));
    }

    #[test]
    fn leaves_slot_outside_select() {
        let raw = r#"<div><slot id="x"></slot></div>"#;
        let out = preprocess(raw).unwrap();
        assert_eq!(out, raw);
    }
}
