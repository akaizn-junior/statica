//! Render planning and renderer selection for mostly-static HTML trees.

use std::collections::HashMap;
use std::collections::HashSet;

use crate::context::CanonicalRoot;
use crate::fragment::FragmentRegistry;
use crate::parse::{Document, Element, Node};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRenderer {
    /// Evaluate a prepared [`RenderPlan`] by stitching static HTML chunks and dynamic ops.
    CompiledPlan,
    /// Clone and mutate the AST, then serialize the resulting document.
    AstMutation,
}

impl PageRenderer {
    #[must_use]
    pub fn select(registry: &FragmentRegistry, doc: &Document, locale: Option<&str>) -> Self {
        if locale.is_none() && compiled_plan_safe(registry, &doc.children) {
            Self::CompiledPlan
        } else {
            Self::AstMutation
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderPlan {
    doctype: Option<String>,
    ops: Vec<Op>,
}

fn compiled_plan_safe(registry: &FragmentRegistry, nodes: &[Node]) -> bool {
    let mut visiting = HashSet::new();
    compiled_plan_safe_nodes(registry, nodes, &mut visiting)
}

fn compiled_plan_safe_nodes(
    registry: &FragmentRegistry,
    nodes: &[Node],
    visiting: &mut HashSet<String>,
) -> bool {
    nodes.iter().all(|node| match node {
        Node::Text(_) | Node::Comment(_) => true,
        Node::Element(el) if el.is_style() || el.is_script() => true,
        Node::Element(el)
            if el.is_slot() && el.attr("id").is_some() && el.attr("name").is_none() =>
        {
            let id = el.attr("id").unwrap_or("");
            let Some(frag) = registry.get(id) else {
                return true;
            };
            if !fragment_compiled_plan_safe(&frag.template.children) {
                return false;
            }
            if !visiting.insert(id.to_string()) {
                return true;
            }
            let safe = compiled_plan_safe_nodes(registry, &frag.template.children, visiting);
            visiting.remove(id);
            safe
        }
        Node::Element(el) => compiled_plan_safe_nodes(registry, &el.children, visiting),
    })
}

fn fragment_compiled_plan_safe(nodes: &[Node]) -> bool {
    nodes.iter().all(|node| match node {
        Node::Text(_) | Node::Comment(_) => true,
        Node::Element(el) if el.is_style() || el.is_script() => false,
        Node::Element(el) => fragment_compiled_plan_safe(&el.children),
    })
}

#[derive(Debug, Clone)]
pub enum Op {
    Static(String),
    AttrTemplate(String),
    TextTemplate(String),
    NamedSlot(String),
    DefaultSlot(Vec<Op>),
    Mount {
        id: String,
        each: Option<String>,
        children: Vec<Op>,
    },
}

impl RenderPlan {
    #[must_use]
    pub fn compile_document(doc: &Document) -> Self {
        Self {
            doctype: doc.doctype.clone(),
            ops: compile_nodes(&doc.children),
        }
    }

    #[must_use]
    pub fn compile_fragment(children: &[Node]) -> Self {
        Self {
            doctype: None,
            ops: compile_nodes(children),
        }
    }

    #[must_use]
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    #[must_use]
    pub fn linked_roots(&self) -> HashSet<String> {
        let mut roots = HashSet::new();
        collect_linked_roots(&self.ops, &mut roots);
        roots
    }

    pub fn write_doctype(&self, out: &mut String) {
        if let Some(doctype) = &self.doctype {
            out.push_str("<!DOCTYPE ");
            out.push_str(doctype);
            out.push_str(">\n");
        }
    }
}

fn collect_linked_roots(ops: &[Op], roots: &mut HashSet<String>) {
    for op in ops {
        match op {
            Op::Static(_) => {}
            Op::AttrTemplate(template) | Op::TextTemplate(template) => {
                collect_template_roots(template, roots);
            }
            Op::NamedSlot(name) => collect_path_root(name, roots),
            Op::DefaultSlot(children) => collect_linked_roots(children, roots),
            Op::Mount { each, children, .. } => {
                if let Some(each) = each {
                    collect_path_root(each, roots);
                }
                collect_linked_roots(children, roots);
            }
        }
    }
}

fn collect_template_roots(template: &str, roots: &mut HashSet<String>) {
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find('}') else {
            break;
        };
        collect_path_root(&rest[..end], roots);
        rest = &rest[end + 1..];
    }
}

fn collect_path_root(path: &str, roots: &mut HashSet<String>) {
    let root = path
        .trim()
        .split('.')
        .find(|part| !part.is_empty())
        .unwrap_or("");
    if !root.is_empty() && root != "this" && CanonicalRoot::from_str(root).is_none() {
        roots.insert(root.to_string());
    }
}

fn compile_nodes(nodes: &[Node]) -> Vec<Op> {
    let mut ops = Vec::new();
    for node in nodes {
        compile_node(node, &mut ops);
    }
    ops
}

fn compile_node(node: &Node, ops: &mut Vec<Op>) {
    match node {
        Node::Text(text) => push_static(ops, text),
        Node::Comment(comment) => push_static(ops, &format!("<!--{comment}-->")),
        Node::Element(el) if is_authoring_link(el) => {}
        Node::Element(el)
            if el.is_slot() && el.attr("id").is_some() && el.attr("name").is_none() =>
        {
            ops.push(Op::Mount {
                id: el.attr("id").unwrap_or("").to_string(),
                each: el.attr("data-each").map(str::to_string),
                children: compile_nodes(&el.children),
            });
        }
        Node::Element(el)
            if el.is_slot() && el.attr("name").is_some() && el.attr("id").is_none() =>
        {
            ops.push(Op::NamedSlot(el.attr("name").unwrap_or("").to_string()));
        }
        Node::Element(el)
            if el.is_slot() && el.attr("name").is_none() && el.attr("id").is_none() =>
        {
            ops.push(Op::DefaultSlot(compile_nodes(&el.children)));
        }
        Node::Element(el) => compile_element(el, ops),
    }
}

fn compile_element(el: &Element, ops: &mut Vec<Op>) {
    push_static(ops, "<");
    push_static(ops, &el.name);
    compile_attrs(el, ops);
    if el.void {
        push_static(ops, " />");
        return;
    }
    push_static(ops, ">");
    if let Some(template) = el.attr("data-t") {
        ops.push(Op::TextTemplate(template.to_string()));
    } else if el.is_script() || el.is_style() {
        for child in &el.children {
            if let Node::Text(text) = child {
                push_static(ops, text);
            }
        }
    } else {
        for child in &el.children {
            compile_node(child, ops);
        }
    }
    push_static(ops, "</");
    push_static(ops, &el.name);
    push_static(ops, ">");
}

fn compile_attrs(el: &Element, ops: &mut Vec<Op>) {
    let translated_attrs: HashMap<&str, &str> = el
        .attrs
        .iter()
        .filter_map(|(name, value)| {
            name.strip_prefix("data-t-")
                .map(|target| (target, value.as_str()))
        })
        .collect();

    for (name, value) in &el.attrs {
        if name == "data-bind" && el.name.eq_ignore_ascii_case("html") {
            continue;
        }
        if name == "data-t" || name.starts_with("data-t-") {
            continue;
        }
        push_static(ops, " ");
        push_static(ops, name);
        push_static(ops, "=\"");
        if let Some(template) = translated_attrs.get(name.as_str()) {
            ops.push(Op::TextTemplate((*template).to_string()));
        } else if value.contains("${") && !(el.is_script() || el.is_style()) {
            ops.push(Op::AttrTemplate(value.clone()));
        } else {
            push_static(ops, &escape_attr(value));
        }
        push_static(ops, "\"");
    }

    for (name, value) in translated_attrs {
        if el.attrs.contains_key(name) {
            continue;
        }
        push_static(ops, " ");
        push_static(ops, name);
        push_static(ops, "=\"");
        ops.push(Op::TextTemplate(value.to_string()));
        push_static(ops, "\"");
    }
}

fn push_static(ops: &mut Vec<Op>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Op::Static(prev)) = ops.last_mut() {
        prev.push_str(text);
    } else {
        ops.push(Op::Static(text.to_string()));
    }
}

fn is_authoring_link(el: &Element) -> bool {
    el.is_link()
        && el.attr("rel").is_some_and(|rel| {
            rel.split_whitespace()
                .any(|part| part == "statica/data" || part == "statica/fragment")
        })
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
