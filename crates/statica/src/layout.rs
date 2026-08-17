//! Page layout projection via `<link rel="statica/layout" href="...">`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::aliases::{self, AliasOptions};
use crate::error::{Error, Result};
use crate::parse::{self, Document, Element, Node, SlotKind, StaticaLinkRel};
use crate::tokens::{ATTR_SLOT, REL_LAYOUT};

#[derive(Debug, Clone, Default)]
struct LayoutProjection {
    default: Vec<Node>,
    named: HashMap<String, Vec<Node>>,
}

impl LayoutProjection {
    fn push_default(&mut self, node: Node) {
        self.default.push(node);
    }

    fn push_named(&mut self, name: impl Into<String>, nodes: impl IntoIterator<Item = Node>) {
        self.named.entry(name.into()).or_default().extend(nodes);
    }

    fn default_children(&self) -> &[Node] {
        &self.default
    }

    fn named_children(&self, name: &str) -> Option<&[Node]> {
        self.named.get(name).map(Vec::as_slice)
    }
}

/// Apply the optional page layout before data, fragment, and binding validation.
pub fn apply_page_layout(
    doc: &mut Document,
    site_root: &Path,
    page_dir: &Path,
    aliases: &AliasOptions,
    page: Option<(&str, &str)>,
) -> Result<Option<PathBuf>> {
    let Some(href) = find_layout_href(doc, page)? else {
        return Ok(None);
    };
    let resolved = aliases::resolve_path(&href, aliases, page, "href")?;
    let path = resolve_layout_path(site_root, page_dir, &resolved, page, &href)?;
    let raw = fs::read_to_string(&path).map_err(|e| Error::read(path.display().to_string(), e))?;
    let file = path.display().to_string();
    let mut layout = parse::parse_document(&raw).map_err(|e| e.in_file(&file, &raw))?;
    rewrite_layout_statica_hrefs(&mut layout, site_root, &path);
    copy_page_html_bind_if_missing(doc, &mut layout);
    let projection = collect_projection(doc);
    fill_layout_slots(&mut layout.children, &projection);
    *doc = layout;
    Ok(Some(path))
}

fn find_layout_href(doc: &Document, page: Option<(&str, &str)>) -> Result<Option<String>> {
    let links = doc.find(|e| matches!(e.statica_link_rel(), Some(StaticaLinkRel::Layout)));
    if links.is_empty() {
        return Ok(None);
    }
    if links.len() > 1 {
        return Err(match page {
            Some((file, source)) => Error::at(
                file,
                source,
                &[
                    &format!("rel=\"{REL_LAYOUT}\""),
                    &format!("rel='{REL_LAYOUT}'"),
                ],
                "a page can declare only one statica layout",
            ),
            None => Error::at_file("<page>", "a page can declare only one statica layout"),
        });
    }
    links[0]
        .attr("href")
        .map(str::to_string)
        .map(Some)
        .ok_or_else(|| match page {
            Some((file, source)) => Error::at(
                file,
                source,
                &[
                    &format!("rel=\"{REL_LAYOUT}\""),
                    &format!("rel='{REL_LAYOUT}'"),
                ],
                "statica layout link is missing `href`",
            ),
            None => Error::at_file("<page>", "statica layout link is missing `href`"),
        })
}

fn resolve_layout_path(
    site_root: &Path,
    page_dir: &Path,
    rel: &str,
    page: Option<(&str, &str)>,
    href: &str,
) -> Result<PathBuf> {
    let joined = aliases::resolve_local_href(site_root, page_dir, rel);
    if let Ok(canon) = joined.canonicalize() {
        return Ok(canon);
    }
    if joined.exists() {
        return Ok(joined);
    }
    let path = joined.display().to_string();
    if let Some((file, source)) = page {
        let dq = format!("href=\"{href}\"");
        let sq = format!("href='{href}'");
        return Err(Error::at(
            file,
            source,
            &[&dq, &sq, href],
            format!("layout path not found: {path}"),
        ));
    }
    Err(Error::at_file(
        path.clone(),
        format!("layout path not found: {path}"),
    ))
}

fn collect_projection(doc: &Document) -> LayoutProjection {
    let mut projection = LayoutProjection::default();
    let Some(html) = root_element(doc, "html") else {
        for node in &doc.children {
            collect_projected_node(node, &mut projection);
        }
        return projection;
    };

    if let Some(head) = child_element(html, "head") {
        let head_nodes = head
            .children
            .iter()
            .filter(|node| !is_layout_link_node(node))
            .cloned()
            .collect::<Vec<_>>();
        projection.push_named("head", head_nodes);
    }
    if let Some(body) = child_element(html, "body") {
        for node in &body.children {
            collect_projected_node(node, &mut projection);
        }
    }
    projection
}

fn copy_page_html_bind_if_missing(page: &Document, layout: &mut Document) {
    let Some(bind) =
        root_element(page, "html").and_then(|html| html.attr(crate::tokens::DATA_BIND))
    else {
        return;
    };
    let Some(layout_html) = root_element_mut(layout, "html") else {
        return;
    };
    if layout_html.attr(crate::tokens::DATA_BIND).is_none() {
        layout_html
            .attrs
            .insert(crate::tokens::DATA_BIND.to_string(), bind.to_string());
    }
}

fn collect_projected_node(node: &Node, projection: &mut LayoutProjection) {
    match node {
        Node::Element(el) if is_layout_link(el) => {}
        Node::Element(el) => {
            if let Some(name) = el
                .attr(ATTR_SLOT)
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                if el.is_template() {
                    projection.push_named(name.to_string(), el.children.clone());
                } else {
                    let mut node = node.clone();
                    if let Node::Element(el) = &mut node {
                        el.attrs.shift_remove(ATTR_SLOT);
                    }
                    projection.push_named(name.to_string(), [node]);
                }
            } else {
                projection.push_default(node.clone());
            }
        }
        Node::Text(text) if text.trim().is_empty() => {}
        _ => projection.push_default(node.clone()),
    }
}

fn fill_layout_slots(nodes: &mut Vec<Node>, projection: &LayoutProjection) {
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
                    fill_layout_slots(&mut el.children, projection);
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

fn rewrite_layout_statica_hrefs(doc: &mut Document, site_root: &Path, layout_path: &Path) {
    let layout_dir = layout_path.parent().unwrap_or_else(|| Path::new("."));
    rewrite_layout_statica_hrefs_in_nodes(&mut doc.children, site_root, layout_dir);
}

fn rewrite_layout_statica_hrefs_in_nodes(nodes: &mut [Node], site_root: &Path, layout_dir: &Path) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if matches!(
            el.statica_link_rel(),
            Some(StaticaLinkRel::Data | StaticaLinkRel::Fragment)
        ) {
            if let Some(href) = el.attr("href").map(str::to_string) {
                if should_rewrite_layout_href(&href) {
                    let abs = resolve_layout_relative_href(layout_dir, &href);
                    let rewritten = abs
                        .strip_prefix(site_root)
                        .ok()
                        .and_then(|p| p.to_str())
                        .map(|p| p.trim_start_matches('/').to_string())
                        .filter(|p| !p.is_empty())
                        .unwrap_or_else(|| abs.display().to_string());
                    el.attrs.insert("href".into(), rewritten);
                }
            }
        }
        rewrite_layout_statica_hrefs_in_nodes(&mut el.children, site_root, layout_dir);
    }
}

fn should_rewrite_layout_href(href: &str) -> bool {
    let href = href.trim();
    !href.is_empty()
        && !href.starts_with('@')
        && !href.starts_with('/')
        && !href.starts_with("http://")
        && !href.starts_with("https://")
        && !crate::funnel::has_template_tokens(href)
}

fn resolve_layout_relative_href(layout_dir: &Path, href: &str) -> PathBuf {
    let path = Path::new(href);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        layout_dir.join(path)
    }
}

fn root_element<'a>(doc: &'a Document, name: &str) -> Option<&'a Element> {
    doc.children.iter().find_map(|node| match node {
        Node::Element(el) if el.name.eq_ignore_ascii_case(name) => Some(el),
        _ => None,
    })
}

fn root_element_mut<'a>(doc: &'a mut Document, name: &str) -> Option<&'a mut Element> {
    doc.children.iter_mut().find_map(|node| match node {
        Node::Element(el) if el.name.eq_ignore_ascii_case(name) => Some(el),
        _ => None,
    })
}

fn child_element<'a>(el: &'a Element, name: &str) -> Option<&'a Element> {
    el.children.iter().find_map(|node| match node {
        Node::Element(child) if child.name.eq_ignore_ascii_case(name) => Some(child),
        _ => None,
    })
}

fn is_layout_link_node(node: &Node) -> bool {
    matches!(node, Node::Element(el) if is_layout_link(el))
}

fn is_layout_link(el: &Element) -> bool {
    matches!(el.statica_link_rel(), Some(StaticaLinkRel::Layout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_default_and_named_slots() {
        let page = parse::parse_document(
            r#"<!doctype html><html><head><title>Page title</title></head><body>
<h1>Hello</h1>
<template slot="sidebar"><p>Side</p></template>
</body></html>"#,
        )
        .unwrap();
        let mut layout = parse::parse_document(
            r#"<!doctype html><html><head><slot name="head"></slot></head><body>
<main><slot></slot></main>
<aside><slot name="sidebar">Fallback</slot></aside>
</body></html>"#,
        )
        .unwrap();
        let projection = collect_projection(&page);
        fill_layout_slots(&mut layout.children, &projection);
        let html = parse::serialize_document(&layout);
        assert!(html.contains("<title>Page title</title>"), "{html}");
        assert!(html.contains("<main><h1>Hello</h1>"), "{html}");
        assert!(html.contains("<aside><p>Side</p></aside>"), "{html}");
        assert!(!html.contains("<template"), "{html}");
    }

    #[test]
    fn named_element_projection_removes_slot_attr() {
        let page = parse::parse_document(
            r#"<html><body><nav slot="nav"><a href="/">Home</a></nav></body></html>"#,
        )
        .unwrap();
        let mut layout = parse::parse_document(
            r#"<html><body><header><slot name="nav"></slot></header></body></html>"#,
        )
        .unwrap();
        let projection = collect_projection(&page);
        fill_layout_slots(&mut layout.children, &projection);
        let html = parse::serialize_document(&layout);
        assert!(html.contains("<nav><a href=\"/\">Home</a></nav>"), "{html}");
        assert!(!html.contains("slot=\"nav\""), "{html}");
    }

    #[test]
    fn copies_page_html_bind_when_layout_has_none() {
        let page =
            parse::parse_document(r#"<html data-bind="{item}"><body></body></html>"#).unwrap();
        let mut layout =
            parse::parse_document(r#"<html><body><slot></slot></body></html>"#).unwrap();
        copy_page_html_bind_if_missing(&page, &mut layout);
        let html = root_element(&layout, "html").unwrap();
        assert_eq!(html.attr(crate::tokens::DATA_BIND), Some("{item}"));
    }
}
