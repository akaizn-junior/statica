//! Named / default `<slot>` filling on the AST.

use serde_json::Value;

use crate::funnel;
use crate::parse::Node;

pub fn fill_named_slots(nodes: &mut Vec<Node>, ctx: &Value) {
    let mut i = 0;
    while i < nodes.len() {
        let replace = match &nodes[i] {
            Node::Element(el) if el.is_slot() && el.attr("name").is_some() && el.attr("id").is_none() => {
                Some(el.attr("name").unwrap_or("").to_string())
            }
            _ => None,
        };
        if let Some(name) = replace {
            // Scope is validated statically; missing/null at runtime → empty.
            let html = match funnel::read_field(ctx, &name) {
                None | Some(Value::Null) => String::new(),
                Some(v) => funnel::value_to_html(v),
            };
            nodes[i] = Node::Text(html);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Element, Node};
    use indexmap::IndexMap;
    use serde_json::json;

    fn named_slot(name: &str, fallback: &str) -> Node {
        let mut attrs = IndexMap::new();
        attrs.insert("name".into(), name.into());
        Node::Element(Element {
            name: "slot".into(),
            attrs,
            children: vec![Node::Text(fallback.into())],
            void: false,
        })
    }

    #[test]
    fn null_and_missing_slots_render_empty() {
        let mut nodes = vec![
            named_slot("label", "fallback"),
            named_slot("missing", "fallback"),
        ];
        fill_named_slots(&mut nodes, &json!({"label": null}));
        assert!(matches!(&nodes[0], Node::Text(t) if t.is_empty()));
        assert!(matches!(&nodes[1], Node::Text(t) if t.is_empty()));
    }

    #[test]
    fn present_slot_renders_value() {
        let mut nodes = vec![named_slot("label", "fallback")];
        fill_named_slots(&mut nodes, &json!({"label": "Go"}));
        assert!(matches!(&nodes[0], Node::Text(t) if t == "Go"));
    }
}
