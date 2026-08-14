//! Build-time search controls and browser-side search index output.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::error::Result;
use crate::parse::{self, AttrMap, Element, Node};
use crate::tokens::{
    DATA_STATICA_SEARCH, DATA_STATICA_SEARCH_CLOSE, DATA_STATICA_SEARCH_META,
    DATA_STATICA_SEARCH_RESULTS, META_PREFIX, SEARCH_CSS_PATH, SEARCH_JS_PATH, SEARCH_RUNTIME_DIR,
    TYPE_SEARCH,
};

const RUNTIME_JS: &str = include_str!("runtime/search.js");
const RUNTIME_CSS: &str = include_str!("runtime/search.css");

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub enabled: bool,
    pub output: String,
    pub limit: usize,
    pub filters: Vec<String>,
    pub url_field: String,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            output: "search.json".into(),
            limit: 10,
            filters: Vec::new(),
            url_field: "url".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SearchEntry {
    url: String,
    title: String,
    section: String,
    text: String,
    excerpt: String,
    meta: Vec<SearchMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SearchUrl(String);

impl SearchUrl {
    pub(crate) fn for_output(out_dir: &Path, path: &Path) -> Self {
        Self(url_for_output(out_dir, path))
    }

    fn into_string(self) -> String {
        self.0
    }
}

#[derive(Debug, Clone, Serialize)]
struct SearchMeta {
    name: String,
    value: String,
}

pub fn rewrite_controls(html: &str, options: &SearchOptions) -> Result<(String, usize)> {
    let mut doc = parse::parse_document(html)?;
    let mut next = 0_usize;
    rewrite_nodes(&mut doc.children, &mut next, options);
    if next == 0 {
        return Ok((html.to_string(), 0));
    }
    append_runtime_script(&mut doc.children);
    Ok((parse::serialize_document(&doc), next))
}

pub fn write_runtime(out_dir: &Path) -> Result<()> {
    let dir = out_dir.join(SEARCH_RUNTIME_DIR);
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
        .map(|html| html.matches(DATA_STATICA_SEARCH).count())
        .sum()
}

pub(crate) fn entry_for_item(item: &Value, url: SearchUrl, collection_id: &str) -> SearchEntry {
    let url = url.into_string();
    let text = item_search_text(item);
    SearchEntry::new(
        url.clone(),
        item_title(item).unwrap_or(url),
        collection_id.replace(['-', '_'], " "),
        text,
        item_meta(item),
    )
}

pub fn write_index(
    out_dir: &Path,
    outputs: &[PathBuf],
    options: &SearchOptions,
    preferred: Vec<SearchEntry>,
) -> Result<()> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for entry in preferred {
        if seen.insert(entry.url.clone()) {
            entries.push(entry);
        }
    }
    for path in outputs
        .iter()
        .filter(|path| is_html(path) && !is_404(out_dir, path))
    {
        let html = fs::read_to_string(path)?;
        let doc = parse::parse_document(&html)?;
        let title = title_text(&doc.children);
        let text = visible_text(&doc.children);
        let url = url_for_output(out_dir, path);
        if seen.insert(url.clone()) {
            entries.push(SearchEntry::new(
                url.clone(),
                if title.is_empty() { url.clone() } else { title },
                section_for_url(&url),
                text,
                meta_values(&doc.children),
            ));
        }
    }
    disambiguate_duplicate_titles(&mut entries);
    let out = out_dir.join(&options.output);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out, serde_json::to_string_pretty(&entries)?)?;
    Ok(())
}

impl SearchEntry {
    fn new(
        url: String,
        title: String,
        section: String,
        text: String,
        meta: Vec<SearchMeta>,
    ) -> Self {
        Self {
            excerpt: excerpt(&text),
            text,
            url,
            title,
            section,
            meta,
        }
    }
}

fn item_title(item: &Value) -> Option<String> {
    let obj = item.as_object()?;
    obj.iter()
        .filter_map(|(key, value)| title_candidate(key, value))
        .max_by_key(|candidate| candidate.score)
        .map(|candidate| candidate.value)
}

struct TitleCandidate {
    value: String,
    score: i32,
}

fn title_candidate(key: &str, value: &Value) -> Option<TitleCandidate> {
    let value = scalar_text(value)?;
    if value.is_empty() || value.chars().count() > 120 {
        return None;
    }
    let key = key.to_ascii_lowercase();
    let base = title_key_score(&key)?;
    Some(TitleCandidate {
        score: base + title_value_score(&value),
        value,
    })
}

fn title_key_score(key: &str) -> Option<i32> {
    match key {
        "headline" | "name" | "label" => Some(100),
        "title" => Some(90),
        "slug" | "filename" | "file" => Some(80),
        _ => None,
    }
}

fn disambiguate_duplicate_titles(entries: &mut [SearchEntry]) {
    let mut counts = HashMap::new();
    for entry in entries.iter() {
        *counts.entry(entry.title.clone()).or_insert(0_usize) += 1;
    }
    for entry in entries {
        if counts.get(&entry.title).copied().unwrap_or_default() > 1 {
            entry.title.clone_from(&entry.url);
        }
    }
}

fn title_value_score(value: &str) -> i32 {
    let mut score = 0;
    if value.chars().any(|ch| ch.is_ascii_digit()) {
        score += 8;
    }
    if value.contains('-') || value.contains('_') || value.contains('/') {
        score += 6;
    }
    if value.split_whitespace().count() >= 3 {
        score += 4;
    }
    if is_generic_title(value) {
        score -= 35;
    }
    score
}

fn is_generic_title(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    let words = value.split_whitespace().collect::<Vec<_>>();
    words.len() <= 3
        && words.iter().any(|word| {
            matches!(
                *word,
                "detail" | "details" | "page" | "record" | "item" | "entry" | "overview"
            )
        })
}

fn item_search_text(value: &Value) -> String {
    let mut out = Vec::new();
    collect_item_text(value, &mut out);
    normalize_ws(&out.join(" "))
}

fn collect_item_text(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Null | Value::Bool(_) => {}
        Value::Number(n) => out.push(n.to_string()),
        Value::String(s) => out.push(strip_html(s)),
        Value::Array(items) => {
            for item in items {
                collect_item_text(item, out);
            }
        }
        Value::Object(obj) => {
            for value in obj.values() {
                collect_item_text(value, out);
            }
        }
    }
}

fn item_meta(item: &Value) -> Vec<SearchMeta> {
    let Some(obj) = item.as_object() else {
        return Vec::new();
    };
    [
        "tags",
        "categories",
        "category",
        "author",
        "published_at",
        "date",
    ]
    .iter()
    .filter_map(|key| {
        let value = obj.get(*key).and_then(meta_text)?;
        Some(SearchMeta {
            name: (*key).into(),
            value,
        })
    })
    .collect()
}

fn meta_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(scalar_text)
                .collect::<Vec<_>>()
                .join(", ");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn strip_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn rewrite_nodes(nodes: &mut Vec<Node>, next: &mut usize, options: &SearchOptions) {
    for node in nodes {
        if let Node::Element(el) = node {
            if is_search_input(el) {
                *next += 1;
                *node = Node::Element(search_control(el, *next, options));
            } else {
                rewrite_nodes(&mut el.children, next, options);
            }
        }
    }
}

fn append_runtime_script(nodes: &mut Vec<Node>) {
    if contains_search_runtime(nodes) {
        return;
    }
    let script = Node::Element(Element {
        name: "script".into(),
        attrs: attrs(&[("type", "module"), ("src", SEARCH_JS_PATH)]),
        void: false,
        children: Vec::new(),
    });
    if let Some(body) = find_body_mut(nodes) {
        body.children.push(script);
    } else {
        nodes.push(script);
    }
}

fn find_body_mut(nodes: &mut [Node]) -> Option<&mut Element> {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if el.name.eq_ignore_ascii_case("body") {
            return Some(el);
        }
        if let Some(body) = find_body_mut(&mut el.children) {
            return Some(body);
        }
    }
    None
}

fn contains_search_runtime(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| {
        let Node::Element(el) = node else {
            return false;
        };
        (el.name.eq_ignore_ascii_case("script") && el.attr("src") == Some(SEARCH_JS_PATH))
            || contains_search_runtime(&el.children)
    })
}

fn is_search_input(el: &Element) -> bool {
    el.name.eq_ignore_ascii_case("input") && el.attr("type").is_some_and(|ty| ty == TYPE_SEARCH)
}

fn search_control(input: &Element, seq: usize, options: &SearchOptions) -> Element {
    let id = input
        .attr("id")
        .map_or_else(|| format!("statica-search-{seq}"), ToOwned::to_owned);
    let placeholder = input.attr("placeholder").unwrap_or("Search");
    let label = input.attr("aria-label").unwrap_or("Search site");
    let index = index_href(&options.output);
    let limit = options.limit.max(1).to_string();
    let filters = options.filters.join(",");
    Element {
        name: "div".into(),
        attrs: attrs(&[("class", "statica-search")]),
        void: false,
        children: vec![
            Node::Element(Element {
                name: "link".into(),
                attrs: attrs(&[("rel", "stylesheet"), ("href", SEARCH_CSS_PATH)]),
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
                    ("title", label),
                ]),
                void: false,
                children: vec![search_icon()],
            }),
            Node::Element(Element {
                name: "dialog".into(),
                attrs: attrs(&[
                    ("id", &id),
                    ("class", "statica-search-modal"),
                    (DATA_STATICA_SEARCH, ""),
                    ("data-index", &index),
                    ("data-limit", &limit),
                    ("data-filters", &filters),
                    ("data-url-field", &options.url_field),
                ]),
                void: false,
                children: vec![
                    Node::Element(Element {
                        name: "div".into(),
                        attrs: attrs(&[("class", "statica-search-head")]),
                        void: false,
                        children: vec![
                            Node::Element(Element {
                                name: "span".into(),
                                attrs: attrs(&[
                                    ("class", "statica-search-input-icon"),
                                    ("aria-hidden", "true"),
                                ]),
                                void: false,
                                children: vec![search_icon()],
                            }),
                            Node::Element(Element {
                                name: "div".into(),
                                attrs: attrs(&[("class", "statica-search-bar")]),
                                void: false,
                                children: vec![Node::Element(Element {
                                    name: "input".into(),
                                    attrs: attrs(&[
                                        ("type", "search"),
                                        ("placeholder", placeholder),
                                        ("aria-label", label),
                                        ("autocomplete", "off"),
                                    ]),
                                    void: true,
                                    children: Vec::new(),
                                })],
                            }),
                            Node::Element(Element {
                                name: "button".into(),
                                attrs: attrs(&[
                                    ("class", "statica-search-close"),
                                    ("type", "button"),
                                    ("aria-label", "Close search"),
                                    (DATA_STATICA_SEARCH_CLOSE, ""),
                                ]),
                                void: false,
                                children: vec![Node::Text("Close".into())],
                            }),
                        ],
                    }),
                    Node::Element(Element {
                        name: "div".into(),
                        attrs: attrs(&[
                            ("class", "statica-search-meta"),
                            (DATA_STATICA_SEARCH_META, ""),
                        ]),
                        void: false,
                        children: Vec::new(),
                    }),
                    Node::Element(Element {
                        name: "div".into(),
                        attrs: attrs(&[
                            ("class", "statica-search-results"),
                            (DATA_STATICA_SEARCH_RESULTS, ""),
                        ]),
                        void: false,
                        children: Vec::new(),
                    }),
                    Node::Element(Element {
                        name: "div".into(),
                        attrs: attrs(&[("class", "statica-search-footer")]),
                        void: false,
                        children: vec![
                            hint("↑", "↓", "to navigate"),
                            hint("↵", "", "to select"),
                            hint("Esc", "", "to exit"),
                        ],
                    }),
                ],
            }),
        ],
    }
}

fn index_href(output: &str) -> String {
    if output.starts_with('/') {
        output.to_string()
    } else {
        format!("/{output}")
    }
}

fn hint(first_key: &str, second_key: &str, text: &str) -> Node {
    let mut children = vec![Node::Element(Element {
        name: "kbd".into(),
        attrs: AttrMap::new(),
        void: false,
        children: vec![Node::Text(first_key.into())],
    })];
    if !second_key.is_empty() {
        children.push(Node::Element(Element {
            name: "kbd".into(),
            attrs: AttrMap::new(),
            void: false,
            children: vec![Node::Text(second_key.into())],
        }));
    }
    children.push(Node::Element(Element {
        name: "span".into(),
        attrs: AttrMap::new(),
        void: false,
        children: vec![Node::Text(text.into())],
    }));
    Node::Element(Element {
        name: "span".into(),
        attrs: attrs(&[("class", "statica-search-hint")]),
        void: false,
        children,
    })
}

fn search_icon() -> Node {
    Node::Element(Element {
        name: "svg".into(),
        attrs: attrs(&[
            ("viewBox", "0 0 24 24"),
            ("width", "18"),
            ("height", "18"),
            ("aria-hidden", "true"),
            ("focusable", "false"),
        ]),
        void: false,
        children: vec![
            Node::Element(Element {
                name: "circle".into(),
                attrs: attrs(&[("cx", "11"), ("cy", "11"), ("r", "7")]),
                void: false,
                children: Vec::new(),
            }),
            Node::Element(Element {
                name: "path".into(),
                attrs: attrs(&[("d", "m16.5 16.5 4 4")]),
                void: false,
                children: Vec::new(),
            }),
        ],
    })
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

fn section_for_url(url: &str) -> String {
    let first = url
        .trim_matches('/')
        .split('/')
        .find(|part| !part.is_empty());
    first
        .map(|part| part.replace('-', " "))
        .unwrap_or_else(|| "home".into())
}

fn meta_values(nodes: &[Node]) -> Vec<SearchMeta> {
    let mut out = Vec::new();
    collect_meta(nodes, &mut out);
    out
}

fn collect_meta(nodes: &[Node], out: &mut Vec<SearchMeta>) {
    for node in nodes {
        if let Node::Element(el) = node {
            if el.name.eq_ignore_ascii_case("meta") {
                if let (Some(name), Some(value)) = (meta_name(el), el.attr("content")) {
                    let value = value.trim();
                    if !value.is_empty() && searchable_meta(name) {
                        out.push(SearchMeta {
                            name: name.to_string(),
                            value: value.to_string(),
                        });
                    }
                }
            }
            collect_meta(&el.children, out);
        }
    }
}

fn meta_name(el: &Element) -> Option<&str> {
    el.attr("name")
        .or_else(|| el.attr("property"))
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

fn searchable_meta(name: &str) -> bool {
    matches!(
        name,
        "description"
            | "author"
            | "keywords"
            | "category"
            | "categories"
            | "tag"
            | "tags"
            | "article:tag"
            | "article:section"
    ) || name.starts_with(META_PREFIX)
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
