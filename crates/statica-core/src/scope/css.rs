//! Scope CSS selectors by appending `[data-s="…"]`.
//!
//! Expects **flattened** CSS (run [`crate::css::transform_css`] first for nesting).
//! Handles `@media` / `@supports` / `@layer` by scoping the block body; leaves
//! `@keyframes` / `@font-face` / `@property` alone.

#[must_use]
pub fn scope_style_text(css: &str, scope_id: &str) -> String {
    scope_block(css, scope_id)
}

fn scope_block(css: &str, scope_id: &str) -> String {
    let mut out = String::with_capacity(css.len() + 64);
    let bytes = css.as_bytes();
    let mut i = 0;
    let n = bytes.len();

    while i < n {
        // Preserve leading whitespace
        let start = i;
        while i < n && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i > start {
            out.push_str(&css[start..i]);
        }
        if i >= n {
            break;
        }

        // Find next `{` at this nesting level for this rule/at-rule
        let prelude_start = i;
        let mut depth = 0usize;
        let mut in_str: Option<u8> = None;
        let mut brace_at = None;
        while i < n {
            let c = bytes[i];
            if let Some(q) = in_str {
                if c == q && bytes[i.saturating_sub(1)] != b'\\' {
                    in_str = None;
                }
                i += 1;
                continue;
            }
            match c {
                b'"' | b'\'' => in_str = Some(c),
                b'{' => {
                    if depth == 0 {
                        brace_at = Some(i);
                        break;
                    }
                    depth += 1;
                }
                b'}' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        let Some(open) = brace_at else {
            out.push_str(&css[prelude_start..]);
            break;
        };

        let prelude = css[prelude_start..open].trim();
        let body_start = open + 1;
        let Some(close) = find_matching_brace(bytes, open) else {
            out.push_str(&css[prelude_start..]);
            break;
        };
        let body = &css[body_start..close];

        if prelude.starts_with('@') {
            let name = at_rule_name(prelude);
            if is_passthrough_at_rule(name) {
                out.push_str(prelude);
                out.push_str(" {");
                out.push_str(body);
                out.push('}');
            } else {
                // @media / @supports / @layer — scope inner rules
                out.push_str(prelude);
                out.push_str(" {");
                out.push_str(&scope_block(body, scope_id));
                out.push('}');
            }
        } else {
            let scoped = scope_selector_list(prelude, scope_id);
            out.push_str(&scoped);
            out.push_str(" {");
            out.push_str(body);
            out.push('}');
        }

        i = close + 1;
    }

    out
}

fn find_matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_str: Option<u8> = None;
    let mut i = open;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = in_str {
            if c == q && (i == 0 || bytes[i - 1] != b'\\') {
                in_str = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' => in_str = Some(c),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn at_rule_name(prelude: &str) -> &str {
    let rest = prelude.trim_start().trim_start_matches('@');
    rest.split_whitespace()
        .next()
        .unwrap_or("")
        .split('(')
        .next()
        .unwrap_or("")
}

fn is_passthrough_at_rule(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "keyframes"
            | "-webkit-keyframes"
            | "-moz-keyframes"
            | "font-face"
            | "property"
            | "counter-style"
            | "font-feature-values"
            | "page"
            | "color-profile"
    )
}

fn scope_selector_list(selectors: &str, scope_id: &str) -> String {
    let marker = format!("[data-s=\"{scope_id}\"]");
    selectors
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.contains(&marker) {
                s.to_string()
            } else {
                format!("{s}{marker}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_simple() {
        let out = scope_style_text(".card { color: red }", "x");
        assert!(out.contains(".card[data-s=\"x\"]"));
    }

    #[test]
    fn scopes_inside_media() {
        let out = scope_style_text(
            "@media (min-width: 40rem) { .card { color: red } }",
            "x",
        );
        assert!(out.contains("@media"));
        assert!(out.contains(".card[data-s=\"x\"]"), "{out}");
        assert!(!out.contains("@media[data-s"), "{out}");
    }

    #[test]
    fn leaves_keyframes() {
        let out = scope_style_text(
            "@keyframes spin { from { opacity: 0 } to { opacity: 1 } }",
            "x",
        );
        assert!(out.contains("@keyframes spin"));
        assert!(!out.contains("from[data-s"), "{out}");
    }
}
