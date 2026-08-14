//! Named / default `<slot>` filling on the AST.

use std::collections::HashMap;

use crate::parse::{Node, SlotKind};
use crate::tokens::ATTR_SLOT;

#[derive(Debug, Clone, Default)]
pub struct SlotProjection {
    default: Vec<Node>,
    named: HashMap<String, Vec<Node>>,
}

impl SlotProjection {
    #[must_use]
    pub fn from_mount_children(children: &[Node]) -> Self {
        let mut projection = Self::default();
        for child in children {
            match child {
                Node::Element(el) => {
                    if let Some(name) = el
                        .attr(ATTR_SLOT)
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    {
                        let mut child = child.clone();
                        if let Node::Element(el) = &mut child {
                            el.attrs.shift_remove(ATTR_SLOT);
                        }
                        projection
                            .named
                            .entry(name.to_string())
                            .or_default()
                            .push(child);
                    } else {
                        projection.default.push(child.clone());
                    }
                }
                Node::Text(text) if text.trim().is_empty() => {
                    projection.default.push(child.clone())
                }
                Node::Text(_) | Node::Comment(_) => projection.default.push(child.clone()),
            }
        }
        projection
    }

    #[must_use]
    pub fn default_children(&self) -> &[Node] {
        &self.default
    }

    #[must_use]
    fn named_children(&self, name: &str) -> Option<&[Node]> {
        self.named.get(name).map(Vec::as_slice)
    }
}

pub fn fill_projection_slots(nodes: &mut Vec<Node>, projection: &SlotProjection) {
    let mut i = 0;
    while i < nodes.len() {
        let slot = match &nodes[i] {
            Node::Element(el) => el.slot_kind(),
            _ => None,
        };
        match slot {
            Some(SlotKind::Default) => {
                replace_slot(nodes, i, projection.default_children());
            }
            Some(SlotKind::Named(name)) => {
                if let Some(children) = projection.named_children(&name) {
                    replace_slot(nodes, i, children);
                } else {
                    unwrap_slot_fallback(nodes, i);
                }
            }
            Some(SlotKind::FragmentMount(_)) | None => {
                if let Node::Element(el) = &mut nodes[i] {
                    fill_projection_slots(&mut el.children, projection);
                }
            }
        }
        i += 1;
    }
}

fn replace_slot(nodes: &mut Vec<Node>, i: usize, children: &[Node]) {
    if children.is_empty() {
        unwrap_slot_fallback(nodes, i);
    } else {
        nodes.splice(i..=i, children.iter().cloned());
    }
}

fn unwrap_slot_fallback(nodes: &mut Vec<Node>, i: usize) {
    if let Node::Element(el) = &mut nodes[i] {
        let fallback = std::mem::take(&mut el.children);
        nodes.splice(i..=i, fallback);
    }
}

pub fn clear_remaining_named_slots(nodes: &mut Vec<Node>) {
    let mut i = 0;
    while i < nodes.len() {
        let clear = matches!(
            &nodes[i],
            Node::Element(el) if matches!(el.slot_kind(), Some(SlotKind::Named(_)))
        );
        if clear {
            if let Node::Element(el) = &mut nodes[i] {
                let fallback = std::mem::take(&mut el.children);
                nodes.splice(i..=i, fallback);
            }
        } else if let Node::Element(el) = &mut nodes[i] {
            clear_remaining_named_slots(&mut el.children);
            i += 1;
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Element, Node};
    use indexmap::IndexMap;

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

    fn element(name: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> Node {
        let mut map = IndexMap::new();
        for (name, value) in attrs {
            map.insert((*name).into(), (*value).into());
        }
        Node::Element(Element {
            name: name.into(),
            attrs: map,
            children,
            void: false,
        })
    }

    #[test]
    fn named_projection_replaces_matching_slot() {
        let mut nodes = vec![named_slot("label", "fallback")];
        let children = vec![element(
            "strong",
            &[("slot", "label")],
            vec![Node::Text("Projected".into())],
        )];
        let projection = SlotProjection::from_mount_children(&children);
        fill_projection_slots(&mut nodes, &projection);
        match &nodes[0] {
            Node::Element(el) => {
                assert_eq!(el.name, "strong");
                assert!(el.attr("slot").is_none());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn named_projection_keeps_fallback_without_matching_child() {
        let mut nodes = vec![named_slot("label", "fallback")];
        let projection = SlotProjection::from_mount_children(&[]);
        fill_projection_slots(&mut nodes, &projection);
        assert!(matches!(&nodes[0], Node::Text(t) if t == "fallback"));
    }

    #[test]
    fn unslotted_children_project_to_default_slot() {
        let mut nodes = vec![element("slot", &[], vec![Node::Text("fallback".into())])];
        let children = vec![element("p", &[], vec![Node::Text("Projected".into())])];
        let projection = SlotProjection::from_mount_children(&children);
        fill_projection_slots(&mut nodes, &projection);
        assert!(matches!(&nodes[0], Node::Element(el) if el.name == "p"));
    }
}
