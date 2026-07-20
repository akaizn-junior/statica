//! Named / default `<slot>` filling on the AST.

use serde_json::Value;

use crate::funnel;
use crate::parse::Node;

pub fn fill_named_slots(nodes: &mut Vec<Node>, ctx: &Value) {
    let mut i = 0;
    while i < nodes.len() {
        let replace = match &nodes[i] {
            Node::Element(el) if el.is_slot() && el.attr("name").is_some() && el.attr("id").is_none() => {
                let name = el.attr("name").unwrap_or("").to_string();
                let fallback = el.children.clone();
                Some((name, fallback))
            }
            _ => None,
        };
        if let Some((name, fallback)) = replace {
            match funnel::get_field(ctx, &name) {
                Some(v) if !v.is_null() => {
                    let html = funnel::value_to_html(v);
                    nodes[i] = Node::Text(html);
                }
                _ => {
                    nodes.splice(i..=i, fallback);
                }
            }
            i += 1;
            continue;
        }
        if let Node::Element(el) = &mut nodes[i] {
            fill_named_slots(&mut el.children, ctx);
        }
        i += 1;
    }
}

/// Project usage children into unnamed `<slot>` (no name, no id).
pub fn fill_default_slots(nodes: &mut Vec<Node>, children: &[Node]) {
    let mut i = 0;
    while i < nodes.len() {
        let is_default = matches!(
            &nodes[i],
            Node::Element(el)
                if el.is_slot() && el.attr("name").is_none() && el.attr("id").is_none()
        );
        if is_default {
            if children.is_empty() {
                // keep fallback children inside the slot by unwrapping
                if let Node::Element(el) = &mut nodes[i] {
                    let fallback = std::mem::take(&mut el.children);
                    nodes.splice(i..=i, fallback);
                }
            } else {
                nodes.splice(i..=i, children.iter().cloned());
            }
            i += 1;
            continue;
        }
        if let Node::Element(el) = &mut nodes[i] {
            fill_default_slots(&mut el.children, children);
        }
        i += 1;
    }
}

pub fn clear_remaining_named_slots(nodes: &mut Vec<Node>) {
    let mut i = 0;
    while i < nodes.len() {
        let clear = matches!(
            &nodes[i],
            Node::Element(el) if el.is_slot() && el.attr("name").is_some()
        );
        if clear {
            if let Node::Element(el) = &mut nodes[i] {
                let fallback = std::mem::take(&mut el.children);
                nodes.splice(i..=i, fallback);
            }
            continue;
        }
        if let Node::Element(el) = &mut nodes[i] {
            clear_remaining_named_slots(&mut el.children);
        }
        i += 1;
    }
}
