use indexmap::IndexMap;

use crate::tokens::{
    DATA_BIND, DATA_EACH, DATA_T, DATA_T_ATTR_PREFIX, REL_DATA, REL_FONT, REL_FRAGMENT, REL_LAYOUT,
};

/// Ordered element attributes.
pub type AttrMap = IndexMap<String, String>;

#[derive(Debug, Clone)]
pub struct Document {
    pub doctype: Option<String>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(String),
    Comment(String),
}

#[derive(Debug, Clone)]
pub struct Element {
    pub name: String,
    pub attrs: AttrMap,
    pub children: Vec<Node>,
    /// True for void/self-closing HTML elements (`link`, `meta`, …).
    pub void: bool,
}

/// Authoring role of a `<slot>` element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotKind {
    /// `<slot name="field">` receives projected children with `slot="field"`.
    Named(String),
    /// `<slot>` receives children passed by the fragment mount.
    Default,
    /// `<slot id="fragment-id">` mounts a statica fragment.
    FragmentMount(String),
}

/// Loop directive from `data-each="path.to.items"` on a fragment mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EachDirective(String);

impl EachDirective {
    #[must_use]
    pub fn new(expr: impl Into<String>) -> Option<Self> {
        let expr = expr.into();
        if expr.trim().is_empty() {
            None
        } else {
            Some(Self(expr))
        }
    }

    #[must_use]
    pub fn expr(&self) -> &str {
        &self.0
    }
}

/// statica-owned link relations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticaLinkRel {
    Data,
    Fragment,
    Font,
    Layout,
}

impl Document {
    #[must_use]
    pub fn new() -> Self {
        Self {
            doctype: None,
            children: Vec::new(),
        }
    }

    /// Depth-first walk; `f` may mutate nodes.
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Node)) {
        for child in &mut self.children {
            walk_node_mut(child, f);
        }
    }

    /// Collect references matching a predicate (immutable).
    pub fn find(&self, mut pred: impl FnMut(&Element) -> bool) -> Vec<&Element> {
        let mut out = Vec::new();
        for child in &self.children {
            find_in_node(child, &mut pred, &mut out);
        }
        out
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Element {
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }

    #[must_use]
    pub fn has_attr_value(&self, name: &str, value: &str) -> bool {
        self.attr(name).is_some_and(|v| v == value)
    }

    #[must_use]
    pub fn is_slot(&self) -> bool {
        self.name.eq_ignore_ascii_case("slot")
    }

    #[must_use]
    pub fn slot_kind(&self) -> Option<SlotKind> {
        if !self.is_slot() {
            return None;
        }
        match (self.attr("name"), self.attr("id")) {
            (Some(name), None) => Some(SlotKind::Named(name.to_string())),
            (None, None) => Some(SlotKind::Default),
            (None, Some(id)) => Some(SlotKind::FragmentMount(id.to_string())),
            (Some(_), Some(_)) => None,
        }
    }

    #[must_use]
    pub fn each_directive(&self) -> Option<EachDirective> {
        self.attr(DATA_EACH)
            .and_then(|expr| EachDirective::new(expr.to_string()))
    }

    #[must_use]
    pub fn bind_directive(&self) -> Option<&str> {
        self.attr(DATA_BIND)
            .map(str::trim)
            .filter(|expr| !expr.is_empty())
    }

    #[must_use]
    pub fn text_directive(&self) -> Option<&str> {
        self.attr(DATA_T)
    }

    #[must_use]
    pub fn translated_attr_target(name: &str) -> Option<&str> {
        name.strip_prefix(DATA_T_ATTR_PREFIX)
    }

    #[must_use]
    pub fn is_translation_attr(name: &str) -> bool {
        name == DATA_T || Self::translated_attr_target(name).is_some()
    }

    #[must_use]
    pub fn statica_link_rel(&self) -> Option<StaticaLinkRel> {
        if !self.is_link() {
            return None;
        }
        let rel = self.attr("rel")?;
        if rel.split_whitespace().any(|part| part == REL_DATA) {
            return Some(StaticaLinkRel::Data);
        }
        if rel.split_whitespace().any(|part| part == REL_FRAGMENT) {
            return Some(StaticaLinkRel::Fragment);
        }
        if rel.split_whitespace().any(|part| part == REL_FONT) {
            return Some(StaticaLinkRel::Font);
        }
        if rel.split_whitespace().any(|part| part == REL_LAYOUT) {
            return Some(StaticaLinkRel::Layout);
        }
        None
    }

    #[must_use]
    pub fn is_template(&self) -> bool {
        self.name.eq_ignore_ascii_case("template")
    }

    #[must_use]
    pub fn is_script(&self) -> bool {
        self.name.eq_ignore_ascii_case("script")
    }

    #[must_use]
    pub fn is_style(&self) -> bool {
        self.name.eq_ignore_ascii_case("style")
    }

    #[must_use]
    pub fn is_link(&self) -> bool {
        self.name.eq_ignore_ascii_case("link")
    }
}

fn walk_node_mut(node: &mut Node, f: &mut impl FnMut(&mut Node)) {
    f(node);
    if let Node::Element(el) = node {
        for child in &mut el.children {
            walk_node_mut(child, f);
        }
    }
}

fn find_in_node<'a>(
    node: &'a Node,
    pred: &mut impl FnMut(&Element) -> bool,
    out: &mut Vec<&'a Element>,
) {
    if let Node::Element(el) = node {
        if pred(el) {
            out.push(el);
        }
        for child in &el.children {
            find_in_node(child, pred, out);
        }
    }
}

/// HTML void elements (no end tag).
#[must_use]
pub fn is_void_element(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}
