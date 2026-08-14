//! Render planning and renderer selection for mostly-static HTML trees.

use std::collections::HashMap;
use std::collections::HashSet;

use crate::context::CanonicalRoot;
use crate::fragment::FragmentRegistry;
use crate::funnel::{self, TemplatePlaceholder, TemplateToken};
use crate::parse::{Document, Element, Node, SlotKind};
use crate::tokens::{ATTR_CLASS, ATTR_SLOT, DATA_BIND};

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
    linked_roots: HashSet<String>,
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
        Node::Element(el) if matches!(el.slot_kind(), Some(SlotKind::FragmentMount(_))) => {
            let Some(SlotKind::FragmentMount(id)) = el.slot_kind() else {
                return true;
            };
            let Some(frag) = registry.get(&id) else {
                return true;
            };
            if !fragment_compiled_plan_safe(&frag.template.children) {
                return false;
            }
            if !visiting.insert(id.clone()) {
                return true;
            }
            let safe = compiled_plan_safe_nodes(registry, &frag.template.children, visiting);
            visiting.remove(&id);
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
    NamedSlot {
        name: String,
        fallback: Vec<Op>,
    },
    DefaultSlot(Vec<Op>),
    SlottedChild {
        name: String,
        children: Vec<Op>,
    },
    Mount {
        id: String,
        each: Option<String>,
        class: Option<String>,
        children: Vec<Op>,
    },
}

impl RenderPlan {
    #[must_use]
    pub fn compile_document(doc: &Document) -> Self {
        let ops = compile_nodes(&doc.children);
        let linked_roots = collect_linked_roots_for_ops(&ops);
        Self {
            doctype: doc.doctype.clone(),
            ops,
            linked_roots,
        }
    }

    #[must_use]
    pub fn compile_fragment(children: &[Node]) -> Self {
        let ops = compile_nodes(children);
        let linked_roots = collect_linked_roots_for_ops(&ops);
        Self {
            doctype: None,
            ops,
            linked_roots,
        }
    }

    #[must_use]
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    #[must_use]
    pub const fn linked_roots(&self) -> &HashSet<String> {
        &self.linked_roots
    }

    pub fn write_doctype(&self, out: &mut String) {
        if let Some(doctype) = &self.doctype {
            out.push_str("<!DOCTYPE ");
            out.push_str(doctype);
            out.push_str(">\n");
        }
    }
}

fn collect_linked_roots_for_ops(ops: &[Op]) -> HashSet<String> {
    let mut roots = HashSet::new();
    collect_linked_roots(ops, &mut roots);
    roots
}

fn collect_linked_roots(ops: &[Op], roots: &mut HashSet<String>) {
    for op in ops {
        match op {
            Op::Static(_) => {}
            Op::AttrTemplate(template) | Op::TextTemplate(template) => {
                collect_template_roots(template, roots);
            }
            Op::NamedSlot { fallback, .. } => collect_linked_roots(fallback, roots),
            Op::DefaultSlot(children) => collect_linked_roots(children, roots),
            Op::SlottedChild { children, .. } => collect_linked_roots(children, roots),
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
    for token in funnel::template_tokens(template) {
        if let TemplateToken::Placeholder(TemplatePlaceholder::Path(path)) = token {
            collect_path_root(path.as_str(), roots);
        }
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
            if el
                .attr(ATTR_SLOT)
                .map(str::trim)
                .is_some_and(|name| !name.is_empty()) =>
        {
            ops.push(Op::SlottedChild {
                name: el.attr(ATTR_SLOT).unwrap_or("").trim().to_string(),
                children: compile_slotted_child(el),
            });
        }
        Node::Element(el) => match el.slot_kind() {
            Some(SlotKind::FragmentMount(id)) => {
                ops.push(Op::Mount {
                    id,
                    each: el.each_directive().map(|each| each.expr().to_string()),
                    class: el.attr(ATTR_CLASS).map(ToOwned::to_owned),
                    children: compile_nodes(&el.children),
                });
            }
            Some(SlotKind::Named(name)) => ops.push(Op::NamedSlot {
                name,
                fallback: compile_nodes(&el.children),
            }),
            Some(SlotKind::Default) => ops.push(Op::DefaultSlot(compile_nodes(&el.children))),
            None => compile_element(el, ops),
        },
    }
}

fn compile_slotted_child(el: &Element) -> Vec<Op> {
    let mut clone = el.clone();
    clone.attrs.shift_remove(ATTR_SLOT);
    let mut ops = Vec::new();
    compile_element(&clone, &mut ops);
    ops
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
    if let Some(template) = el.text_directive() {
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
            Element::translated_attr_target(name).map(|target| (target, value.as_str()))
        })
        .collect();

    for (name, value) in &el.attrs {
        let is_html_bind = name == DATA_BIND && el.name.eq_ignore_ascii_case("html");
        if !is_html_bind && !Element::is_translation_attr(name) {
            push_static(ops, " ");
            push_static(ops, name);
            push_static(ops, "=\"");
            if let Some(template) = translated_attrs.get(name.as_str()) {
                ops.push(Op::TextTemplate((*template).to_string()));
            } else if funnel::has_template_tokens(value) && !(el.is_script() || el.is_style()) {
                ops.push(Op::AttrTemplate(value.clone()));
            } else {
                push_static(ops, &escape_attr(value));
            }
            push_static(ops, "\"");
        }
    }

    for (name, value) in translated_attrs {
        if !el.attrs.contains_key(name) {
            push_static(ops, " ");
            push_static(ops, name);
            push_static(ops, "=\"");
            ops.push(Op::TextTemplate(value.to_string()));
            push_static(ops, "\"");
        }
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
    el.statica_link_rel().is_some()
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
