//! html5ever → owned [`Document`] AST.

use html5ever::parse_document as html5ever_parse;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::error::{Error, Result};
use crate::parse::ast::{is_void_element, AttrMap, Document, Element, Node};
use crate::parse::{normalize, pre};

/// Parse a full HTML document (or anything html5ever accepts as a document).
pub fn parse_document(input: &str) -> Result<Document> {
    let input = pre::preprocess(input)?;
    let mut doc = parse_document_inner(&input)?;
    normalize::normalize_authoring_nodes(&mut doc.children);
    Ok(doc)
}

fn parse_document_inner(input: &str) -> Result<Document> {
    let dom = html5ever_parse(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut input.as_bytes())
        .map_err(|e| Error::msg(format!("html parse failed: {e}")))?;
    Ok(from_rcdom(&dom))
}

/// Parse an HTML fragment (component file body, template contents, …).
pub fn parse_fragment(input: &str) -> Result<Document> {
    let input = pre::preprocess(input)?;
    let mut doc = parse_document_inner(&input)?;
    normalize::normalize_authoring_nodes(&mut doc.children);
    if let Some(body_children) = peel_body_children(&mut doc) {
        doc.children = body_children;
        doc.doctype = None;
    }
    Ok(doc)
}

fn peel_body_children(doc: &mut Document) -> Option<Vec<Node>> {
    let html = doc.children.iter_mut().find_map(|n| match n {
        Node::Element(el) if el.name.eq_ignore_ascii_case("html") => Some(el),
        _ => None,
    })?;
    let body = html.children.iter_mut().find_map(|n| match n {
        Node::Element(el) if el.name.eq_ignore_ascii_case("body") => Some(el),
        _ => None,
    })?;
    if !body.children.is_empty() {
        return Some(std::mem::take(&mut body.children));
    }
    let head = html.children.iter_mut().find_map(|n| match n {
        Node::Element(el) if el.name.eq_ignore_ascii_case("head") => Some(el),
        _ => None,
    })?;
    if head
        .children
        .iter()
        .any(|c| matches!(c, Node::Element(e) if e.is_template() || e.is_link() || e.is_script()))
    {
        return Some(std::mem::take(&mut head.children));
    }
    None
}

fn from_rcdom(dom: &RcDom) -> Document {
    let mut doc = Document::new();
    for child in dom.document.children.borrow().iter() {
        match &child.data {
            NodeData::Doctype { name, .. } => {
                doc.doctype = Some(name.to_string());
            }
            _ => {
                if let Some(node) = convert_handle(child) {
                    doc.children.push(node);
                }
            }
        }
    }
    doc
}

fn convert_handle(handle: &Handle) -> Option<Node> {
    match &handle.data {
        NodeData::Text { contents } => {
            let t = contents.borrow().to_string();
            if t.is_empty() {
                None
            } else {
                Some(Node::Text(t))
            }
        }
        NodeData::Comment { contents } => Some(Node::Comment(contents.to_string())),
        NodeData::Element {
            name,
            attrs,
            template_contents,
            ..
        } => {
            let tag = name.local.to_string();
            let mut map = AttrMap::new();
            for attr in attrs.borrow().iter() {
                map.insert(attr.name.local.to_string(), attr.value.to_string());
            }
            let mut children = Vec::new();
            let tmpl = template_contents.borrow();
            if let Some(ref tmpl_handle) = *tmpl {
                for child in tmpl_handle.children.borrow().iter() {
                    if let Some(n) = convert_handle(child) {
                        children.push(n);
                    }
                }
            } else {
                for child in handle.children.borrow().iter() {
                    if let Some(n) = convert_handle(child) {
                        children.push(n);
                    }
                }
            }
            Some(Node::Element(Element {
                void: is_void_element(&tag),
                name: tag,
                attrs: map,
                children,
            }))
        }
        NodeData::Document
        | NodeData::Doctype { .. }
        | NodeData::ProcessingInstruction { .. } => None,
    }
}

#[cfg(test)]
mod select_slot_tests {
    use super::*;

    #[test]
    fn slot_inside_select_is_preserved() {
        let html = r#"<select name="country" required><slot id="select-option" data-each="items"></slot></select>"#;
        let doc = parse_fragment(html).unwrap();
        let select = doc.find(|e| e.name.eq_ignore_ascii_case("select"));
        assert_eq!(select.len(), 1);
        let children = &select[0].children;
        assert!(
            children.iter().any(|n| matches!(n, crate::parse::Node::Element(e) if e.is_slot())),
            "expected slot child, got: {:?}",
            children.iter().map(|n| match n {
                crate::parse::Node::Element(e) => e.name.clone(),
                crate::parse::Node::Text(t) => format!("text:{t}"),
                crate::parse::Node::Comment(c) => format!("comment:{c}"),
            }).collect::<Vec<_>>()
        );
        let slot = children.iter().find_map(|n| match n {
            crate::parse::Node::Element(e) if e.is_slot() => Some(e),
            _ => None,
        }).unwrap();
        assert_eq!(slot.attr("id"), Some("select-option"));
        assert_eq!(slot.attr("data-each"), Some("items"));
    }

    #[test]
    fn slot_inside_optgroup_is_preserved() {
        let html = r#"<select><optgroup label="Countries"><slot id="select-option" data-each="items"></slot></optgroup></select>"#;
        let doc = parse_fragment(html).unwrap();
        let optgroup = doc.find(|e| e.name.eq_ignore_ascii_case("optgroup"));
        assert_eq!(optgroup.len(), 1);
        assert!(optgroup[0].children.iter().any(|n| matches!(
            n,
            crate::parse::Node::Element(e) if e.is_slot()
        )));
    }
}
