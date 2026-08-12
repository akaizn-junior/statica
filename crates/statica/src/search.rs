//! Build-time search controls and browser-side search index output.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Result;
use crate::parse::{self, AttrMap, Element, Node};

const RUNTIME_JS: &str = include_str!("runtime/search.js");
const RUNTIME_CSS: &str = include_str!("runtime/search.css");

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub enabled: bool,
    pub output: String,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            output: "search.json".into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct SearchEntry {
    url: String,
    title: String,
    text: String,
    excerpt: String,
}

pub fn rewrite_controls(html: &str, default_index: &str) -> Result<(String, usize)> {
    let mut doc = parse::parse_document(html)?;
    let mut next = 0_usize;
    rewrite_nodes(&mut doc.children, &mut next, default_index);
    if next == 0 {
        return Ok((html.to_string(), 0));
    }
    Ok((parse::serialize_document(&doc), next))
}

pub fn write_runtime(out_dir: &Path) -> Result<()> {
    let dir = out_dir.join("statica");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("search.js"), RUNTIME_JS)?;
    fs::write(dir.join("search.css"), RUNTIME_CSS)?;
    Ok(())
}

#[must_use]
pub fn count_controls_in_outputs(outputs: &[PathBuf]) -> usize {
    outputs
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .map(|html| html.matches("data-statica-search").count())
        .sum()
}

pub fn write_index(out_dir: &Path, outputs: &[PathBuf], options: &SearchOptions) -> Result<()> {
    let mut entries = Vec::new();
    for path in outputs
        .iter()
        .filter(|path| is_html(path) && !is_404(out_dir, path))
    {
        let html = fs::read_to_string(path)?;
        let doc = parse::parse_document(&html)?;
        let title = title_text(&doc.children);
        let text = visible_text(&doc.children);
        entries.push(SearchEntry {
            url: url_for_output(out_dir, path),
            title: if title.is_empty() {
                url_for_output(out_dir, path)
            } else {
                title
            },
            excerpt: excerpt(&text),
            text,
        });
    }
    let out = out_dir.join(&options.output);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out, serde_json::to_string_pretty(&entries)?)?;
    Ok(())
}

fn rewrite_nodes(nodes: &mut Vec<Node>, next: &mut usize, default_index: &str) {
    for node in nodes {
        if let Node::Element(el) = node {
            if is_search_input(el) {
                *next += 1;
                *node = Node::Element(search_control(el, *next, default_index));
            } else {
                rewrite_nodes(&mut el.children, next, default_index);
            }
        }
    }
}

fn is_search_input(el: &Element) -> bool {
    el.name.eq_ignore_ascii_case("input")
        && el.attr("type").is_some_and(|ty| ty == "statica/search")
}

fn search_control(input: &Element, seq: usize, default_index: &str) -> Element {
    let id = input
        .attr("id")
        .map_or_else(|| format!("statica-search-{seq}"), ToOwned::to_owned);
    let placeholder = input.attr("placeholder").unwrap_or("Search");
    let index = input.attr("data-index").unwrap_or(default_index);
    let limit = input.attr("data-limit").unwrap_or("10");
    let label = input.attr("aria-label").unwrap_or("Search site");
    Element {
        name: "div".into(),
        attrs: attrs(&[("class", "statica-search")]),
        void: false,
        children: vec![
            Node::Element(Element {
                name: "link".into(),
                attrs: attrs(&[("rel", "stylesheet"), ("href", "/statica/search.css")]),
                void: true,
                children: Vec::new(),
            }),
            Node::Element(Element {
                name: "button".into(),
                attrs: attrs(&[
                    ("class", "statica-search-trigger"),
                    ("type", "button"),
                    ("aria-controls", &id),
                    ("aria-label", label),
                ]),
                void: false,
                children: vec![Node::Text(placeholder.into())],
            }),
            Node::Element(Element {
                name: "dialog".into(),
                attrs: attrs(&[
                    ("id", &id),
                    ("class", "statica-search-modal"),
                    ("data-statica-search", ""),
                    ("data-index", index),
                    ("data-limit", limit),
                ]),
                void: false,
                children: vec![
                    Node::Element(Element {
                        name: "div".into(),
                        attrs: attrs(&[("class", "statica-search-bar")]),
                        void: false,
                        children: vec![
                            Node::Element(Element {
                                name: "input".into(),
                                attrs: attrs(&[
                                    ("type", "search"),
                                    ("placeholder", placeholder),
                                    ("aria-label", label),
                                    ("autocomplete", "off"),
                                ]),
                                void: true,
                                children: Vec::new(),
                            }),
                            Node::Element(Element {
                                name: "button".into(),
                                attrs: attrs(&[
                                    ("type", "button"),
                                    ("data-statica-search-close", ""),
                                ]),
                                void: false,
                                children: vec![Node::Text("Close".into())],
                            }),
                        ],
                    }),
                    Node::Element(Element {
                        name: "div".into(),
                        attrs: attrs(&[
                            ("class", "statica-search-results"),
                            ("data-statica-search-results", ""),
                        ]),
                        void: false,
                        children: Vec::new(),
                    }),
                    Node::Element(Element {
                        name: "script".into(),
                        attrs: attrs(&[("type", "module"), ("src", "/statica/search.js")]),
                        void: false,
                        children: Vec::new(),
                    }),
                ],
            }),
        ],
    }
}

fn attrs(pairs: &[(&str, &str)]) -> AttrMap {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn is_html(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("html")
}

fn is_404(out_dir: &Path, path: &Path) -> bool {
    path.strip_prefix(out_dir)
        .is_ok_and(|rel| rel == Path::new("404.html") || rel == Path::new("404").join("index.html"))
}

fn url_for_output(out_dir: &Path, path: &Path) -> String {
    let Ok(rel) = path.strip_prefix(out_dir) else {
        return "/".into();
    };
    if rel == Path::new("index.html") {
        return "/".into();
    }
    let mut parts = rel
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect::<Vec<_>>();
    if parts.last() == Some(&"index.html") {
        parts.pop();
    }
    format!("/{}/", parts.join("/"))
}

fn title_text(nodes: &[Node]) -> String {
    find_element_text(nodes, "title").unwrap_or_default()
}

fn find_element_text(nodes: &[Node], name: &str) -> Option<String> {
    for node in nodes {
        if let Node::Element(el) = node {
            if el.name.eq_ignore_ascii_case(name) {
                return Some(normalize_ws(&text_from_nodes(&el.children, false)));
            }
            if let Some(found) = find_element_text(&el.children, name) {
                return Some(found);
            }
        }
    }
    None
}

fn visible_text(nodes: &[Node]) -> String {
    normalize_ws(&text_from_nodes(nodes, true))
}

fn text_from_nodes(nodes: &[Node], skip_hidden: bool) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(text) => {
                out.push_str(text);
                out.push(' ');
            }
            Node::Comment(_) => {}
            Node::Element(el) if skip_hidden && hidden_from_search(el) => {}
            Node::Element(el) => out.push_str(&text_from_nodes(&el.children, skip_hidden)),
        }
    }
    out
}

fn hidden_from_search(el: &Element) -> bool {
    matches!(
        el.name.to_ascii_lowercase().as_str(),
        "script" | "style" | "template" | "noscript" | "dialog"
    )
}

fn normalize_ws(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn excerpt(text: &str) -> String {
    const MAX: usize = 180;
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    text.chars().take(MAX).collect::<String>()
}
