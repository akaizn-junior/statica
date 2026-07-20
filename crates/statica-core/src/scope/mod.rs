//! CSS / JS fragment scoping (`data-s`) + `$` injection from `runtime/statica.js`.

mod css;
mod js;

use crate::parse::{Document, Element, Node};
use crate::runtime::STATICA_JS;

pub use css::scope_style_text;
pub use js::wrap_script_with_scope;

/// Stamp `data-s="{scope_id}"` on elements (not style/script/slot).
pub fn apply_scope_to_nodes(nodes: &mut [Node], scope_id: &str) {
    for node in nodes {
        if let Node::Element(el) = node {
            if !(el.is_style() || el.is_script() || el.is_slot()) {
                el.attrs
                    .entry("data-s".into())
                    .or_insert_with(|| scope_id.to_string());
            }
            if el.is_style() {
                if let Some(Node::Text(css)) = el.children.first_mut() {
                    match crate::css::transform_and_scope(css, scope_id) {
                        Ok(ready) => *css = ready,
                        Err(_) => {
                            // Fallback: scope flattened-looking CSS without full transform
                            *css = scope_style_text(css, scope_id);
                        }
                    }
                }
            }
            apply_scope_to_nodes(&mut el.children, scope_id);
        }
    }
}

/// Rewrite fragment scripts that reference `$` to bind the inlined scope helper.
pub fn rewrite_scripts_in_nodes(nodes: &mut [Node], scope_id: &str) {
    for node in nodes {
        if let Node::Element(el) = node {
            if el.is_script() {
                let body = el
                    .children
                    .iter()
                    .filter_map(|c| match c {
                        Node::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                if body.contains("$.") || body.contains("$(") || body.contains("__staticaScope") {
                    if body.contains("__staticaScope") || body.contains("__statica.scope") {
                        continue;
                    }
                    el.attrs.shift_remove("type"); // classic script so currentScript works
                    el.attrs
                        .insert("data-statica-scope".into(), scope_id.to_string());
                    let wrapped = wrap_script_with_scope(&body, scope_id);
                    el.children = vec![Node::Text(wrapped)];
                }
            } else {
                rewrite_scripts_in_nodes(&mut el.children, scope_id);
            }
        }
    }
}

pub fn dedupe_helpers_in_document(doc: &mut Document) {
    let mut seen = false;
    walk_scripts(&mut doc.children, &mut |el: &mut Element| {
        let scope = el
            .attr("data-statica-scope")
            .unwrap_or("")
            .to_string();
        if let Some(Node::Text(body)) = el.children.first_mut() {
            if body.contains("function __staticaScope") || body.contains("__statica.scope") {
                if seen {
                    if let Some(idx) = body.find("(function (scriptEl)") {
                        *body = body[idx..].to_string();
                    } else if let Some(idx) = body.find("const $ =") {
                        *body = format!(
                            "const $ = __statica.scope(document.currentScript, \"{scope}\");\n{}",
                            &body[idx..]
                        );
                    }
                } else {
                    seen = true;
                }
            }
        }
    });
    let _ = STATICA_JS;
}

pub fn dedupe_styles_in_document(doc: &mut Document) {
    let mut seen = std::collections::HashSet::new();
    dedupe_styles(&mut doc.children, &mut seen);
}

fn dedupe_styles(nodes: &mut Vec<Node>, seen: &mut std::collections::HashSet<String>) {
    let mut i = 0;
    while i < nodes.len() {
        let key = match &nodes[i] {
            Node::Element(el) if el.is_style() => style_scope_key(el),
            _ => None,
        };
        if let Some(key) = key {
            if !seen.insert(key) {
                nodes.remove(i);
                continue;
            }
        }
        if let Node::Element(el) = &mut nodes[i] {
            dedupe_styles(&mut el.children, seen);
        }
        i += 1;
    }
}

fn style_scope_key(el: &Element) -> Option<String> {
    let css = el.children.iter().find_map(|c| match c {
        Node::Text(t) => Some(t.as_str()),
        _ => None,
    })?;
    // first data-s="…" occurrence
    let bytes = css.as_bytes();
    let marker = b"[data-s=\"";
    if let Some(pos) = css.find("[data-s=\"") {
        let start = pos + marker.len();
        let end = css[start..].find('"')? + start;
        return Some(css[start..end].to_string());
    }
    let _ = bytes;
    Some(css.to_string())
}

fn walk_scripts(nodes: &mut [Node], f: &mut impl FnMut(&mut Element)) {
    for node in nodes {
        if let Node::Element(el) = node {
            if el.is_script() {
                f(el);
            }
            walk_scripts(&mut el.children, f);
        }
    }
}
