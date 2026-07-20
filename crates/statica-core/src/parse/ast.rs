use indexmap::IndexMap;

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
    pub fn find<'a>(&'a self, mut pred: impl FnMut(&Element) -> bool) -> Vec<&'a Element> {
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
