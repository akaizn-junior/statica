//! Serialize owned AST → HTML string (no regex).

use crate::parse::ast::{Document, Element, Node};

#[must_use]
pub fn serialize_document(doc: &Document) -> String {
    let mut out = String::with_capacity(4096);
    if let Some(dt) = &doc.doctype {
        out.push_str("<!DOCTYPE ");
        out.push_str(dt);
        out.push_str(">\n");
    }
    for child in &doc.children {
        serialize_node(child, &mut out);
    }
    out
}

#[must_use]
pub fn serialize_nodes(nodes: &[Node]) -> String {
    let mut out = String::new();
    for n in nodes {
        serialize_node(n, &mut out);
    }
    out
}

fn serialize_node(node: &Node, out: &mut String) {
    match node {
        Node::Text(t) => out.push_str(t),
        Node::Comment(c) => {
            out.push_str("<!--");
            out.push_str(c);
            out.push_str("-->");
        }
        Node::Element(el) => serialize_element(el, out),
    }
}

fn serialize_element(el: &Element, out: &mut String) {
    out.push('<');
    out.push_str(&el.name);
    for (k, v) in &el.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&escape_attr(v));
        out.push('"');
    }
    if el.void {
        out.push_str(" />");
        return;
    }
    out.push('>');
    // Raw text elements: do not escape children.
    let raw = el.is_script() || el.is_style();
    for child in &el.children {
        if raw {
            if let Node::Text(t) = child {
                out.push_str(t);
                continue;
            }
        }
        serialize_node(child, out);
    }
    out.push_str("</");
    out.push_str(&el.name);
    out.push('>');
}

fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}
