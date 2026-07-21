//! Post-parse AST normalization for pre-pass authoring rewrites.

use crate::parse::{Element, Node};

/// Lower pre-pass carriers back to authoring nodes (`<script type="statica/slot">` → `<slot>`).
pub fn normalize_authoring_nodes(nodes: &mut [Node]) {
    for node in nodes {
        if let Node::Element(el) = node {
            if el.is_statica_slot_carrier() {
                el.name = "slot".to_string();
                el.void = false;
                el.attrs.shift_remove("type");
            }
            normalize_authoring_nodes(&mut el.children);
        }
    }
}

impl Element {
    #[must_use]
    pub fn is_statica_slot_carrier(&self) -> bool {
        self.is_script() && self.attr("type").is_some_and(|t| t == "statica/slot")
    }
}
