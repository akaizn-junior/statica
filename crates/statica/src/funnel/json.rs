//! Local funnel sources via `<link rel="statica/data" href id>`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::aliases::{self, AliasOptions};
use crate::content;
use crate::context::CanonicalRoot;
use crate::error::{Error, Result};
use crate::parse::escape_text;
use crate::parse::{Document, Element, Node, StaticaLinkRel};
use crate::tokens::{missing_data_source_message, rel_double_quoted, rel_single_quoted, REL_DATA};

use std::path::Component;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DataSource {
    pub id: String,
    pub kind: content::DataKind,
    pub path: PathBuf,
    pub data: Arc<content::DataSet>,
}

impl DataSource {
    #[must_use]
    pub fn value(&self) -> Value {
        self.data.to_value()
    }

    #[must_use]
    pub fn array(&self) -> Option<Vec<Value>> {
        self.data.as_array()
    }
}

pub fn document_has_dynamic_data(doc: &Document) -> bool {
    doc.find(is_data_link).into_iter().any(|el| {
        el.attr("href")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some_and(super::template::has_template_tokens)
    })
}

/// Whether a specific funnel `<link rel="statica/data" id="…">` has a dynamic `href`.
pub fn data_link_has_dynamic_href(doc: &Document, id: &str) -> bool {
    doc.find(is_data_link).into_iter().any(|el| {
        el.attr("id") == Some(id)
            && el
                .attr("href")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some_and(super::template::has_template_tokens)
    })
}

pub fn load_data_from_document(
    doc: &Document,
    site_root: &Path,
    page_dir: &Path,
    cache: &mut HashMap<PathBuf, Arc<content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<HashMap<String, DataSource>> {
    load_data_links(
        doc,
        site_root,
        page_dir,
        cache,
        aliases,
        site,
        DataLinkFilter::StaticOnly,
    )
}

/// Load funnel sources whose `href` contains dynamic attribute placeholders.
pub fn load_dynamic_data_from_document(
    doc: &Document,
    site_root: &Path,
    page_dir: &Path,
    cache: &mut HashMap<PathBuf, Arc<content::DataSet>>,
    aliases: &AliasOptions,
    context: &Value,
    site: Option<(&str, &str)>,
) -> Result<HashMap<String, DataSource>> {
    load_data_links(
        doc,
        site_root,
        page_dir,
        cache,
        aliases,
        site,
        DataLinkFilter::DynamicOnly { context },
    )
}

pub fn load_dynamic_data_from_fragment_template(
    doc: &Document,
    template_id: &str,
    site_root: &Path,
    page_dir: &Path,
    cache: &mut HashMap<PathBuf, Arc<content::DataSet>>,
    aliases: &AliasOptions,
    document_context: &Value,
    template_context: &Value,
    site: Option<(&str, &str)>,
) -> Result<HashMap<String, DataSource>> {
    load_data_links(
        doc,
        site_root,
        page_dir,
        cache,
        aliases,
        site,
        DataLinkFilter::FragmentDynamic {
            template_id,
            document_context,
            template_context,
        },
    )
}

#[derive(Clone, Copy)]
enum DataLinkFilter<'a> {
    StaticOnly,
    DynamicOnly {
        context: &'a Value,
    },
    FragmentDynamic {
        template_id: &'a str,
        document_context: &'a Value,
        template_context: &'a Value,
    },
}

struct DataLinkRef<'a> {
    el: &'a Element,
    in_template: bool,
}

fn load_data_links(
    doc: &Document,
    site_root: &Path,
    page_dir: &Path,
    cache: &mut HashMap<PathBuf, Arc<content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
    filter: DataLinkFilter<'_>,
) -> Result<HashMap<String, DataSource>> {
    let mut out = HashMap::new();
    let links = data_links_for_filter(doc, filter);
    for link in links {
        let el = link.el;
        let rel_dq = rel_double_quoted(REL_DATA);
        let rel_sq = rel_single_quoted(REL_DATA);
        let id = match el.attr("id").map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => id.to_string(),
            None => {
                return Err(site_err(
                    site,
                    &[&rel_dq, &rel_sq],
                    format!(
                        "{REL_DATA} link is missing `id`; add an id such as <link rel=\"{REL_DATA}\" href=\"...\" id=\"posts\"> so templates can refer to the data"
                    ),
                ));
            }
        };
        if CanonicalRoot::from_str(&id).is_some() {
            let id_dq = format!("id=\"{id}\"");
            let id_sq = format!("id='{id}'");
            return Err(site_err(
                site,
                &[&id_dq, &id_sq],
                format!(
                    "{REL_DATA} id `{id}` conflicts with canonical page context; rename this data source to something project-specific, such as `posts` or `site`"
                ),
            ));
        }
        let Some(href) = el.attr("href").map(str::trim).filter(|s| !s.is_empty()) else {
            let id_dq = format!("id=\"{id}\"");
            return Err(site_err(
                site,
                &[&rel_dq, id_dq.as_str()],
                format!(
                    "{REL_DATA} link `{id}` is missing `href`; point it at a file or explicit glob, for example href=\"../content/{id}.json\""
                ),
            ));
        };
        let href = aliases::resolve_path(href, aliases, site, "href")?;
        let has_dynamic_tokens = super::template::has_template_tokens(&href);
        match filter {
            DataLinkFilter::StaticOnly if has_dynamic_tokens => continue,
            DataLinkFilter::DynamicOnly { .. } | DataLinkFilter::FragmentDynamic { .. }
                if !has_dynamic_tokens =>
            {
                continue;
            }
            _ => {}
        }
        let href = materialize_dynamic_href(href, filter, link.in_template, site)?;
        let explicit_kind = match el
            .attr("type")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(raw) => Some(content::DataKind::from_type_attr(raw).ok_or_else(|| {
                let type_dq = format!("type=\"{raw}\"");
                let type_sq = format!("type='{raw}'");
                site_err(
                    site,
                    &[&type_dq, &type_sq, raw],
                    format!(
                        "unsupported {REL_DATA} type `{raw}`; use JSON, JSONL/NDJSON, CSV, Markdown, plain text, or omit `type` and let statica infer it from the file extension"
                    ),
                )
            })?),
            None => None,
        };
        let cache_key = content_cache_key(site_root, page_dir, &href);
        let (kind, data) = if explicit_kind.is_none() {
            if let Some(data) = cache.get(&cache_key) {
                (inferred_data_kind(&href), Arc::clone(data))
            } else {
                let parsed = load_link_content(site_root, page_dir, &href, explicit_kind, site)?;
                let data = Arc::new(parsed.data);
                cache.insert(cache_key.clone(), Arc::clone(&data));
                (parsed.kind, data)
            }
        } else {
            // `type` changes how bytes are parsed, so explicit links stay out of the path cache.
            let parsed = load_link_content(site_root, page_dir, &href, explicit_kind, site)?;
            (parsed.kind, Arc::new(parsed.data))
        };
        out.insert(
            id.clone(),
            DataSource {
                id,
                kind,
                path: cache_key,
                data,
            },
        );
    }
    Ok(out)
}

fn data_links_for_filter<'a>(
    doc: &'a Document,
    filter: DataLinkFilter<'_>,
) -> Vec<DataLinkRef<'a>> {
    match filter {
        DataLinkFilter::StaticOnly | DataLinkFilter::DynamicOnly { .. } => doc
            .find(is_data_link)
            .into_iter()
            .map(|el| DataLinkRef {
                el,
                in_template: false,
            })
            .collect(),
        DataLinkFilter::FragmentDynamic { template_id, .. } => {
            let mut out = Vec::new();
            for node in &doc.children {
                collect_fragment_data_links(node, template_id, false, &mut out);
            }
            out
        }
    }
}

fn collect_fragment_data_links<'a>(
    node: &'a Node,
    template_id: &str,
    in_template: bool,
    out: &mut Vec<DataLinkRef<'a>>,
) {
    let Node::Element(el) = node else {
        return;
    };
    if is_data_link(el) {
        out.push(DataLinkRef { el, in_template });
    }
    let child_in_template = in_template || (el.is_template() && el.attr("id") == Some(template_id));
    for child in &el.children {
        collect_fragment_data_links(child, template_id, child_in_template, out);
    }
}

fn materialize_dynamic_href(
    href: String,
    filter: DataLinkFilter<'_>,
    in_template: bool,
    site: Option<(&str, &str)>,
) -> Result<String> {
    let context = match filter {
        DataLinkFilter::DynamicOnly { context } => Some(context),
        DataLinkFilter::FragmentDynamic {
            document_context,
            template_context,
            ..
        } => Some(if in_template {
            template_context
        } else {
            document_context
        }),
        DataLinkFilter::StaticOnly => None,
    };
    match context {
        Some(context) => {
            super::template::expand_dynamic_attribute(&href, &context).map_err(|err| {
                let authored = err.authored();
                site_err(site, &[&authored, &href], err.message())
            })
        }
        None => Ok(href),
    }
}

fn load_link_content(
    site_root: &Path,
    page_dir: &Path,
    href: &str,
    explicit_kind: Option<content::DataKind>,
    site: Option<(&str, &str)>,
) -> Result<content::LoadedContent> {
    content::load_content(site_root, page_dir, href, explicit_kind).map_err(|e| match site {
        Some((file, source)) => {
            let href_dq = format!("href=\"{href}\"");
            let href_sq = format!("href='{href}'");
            Error::at(file, source, &[&href_dq, &href_sq, href], e.to_string())
        }
        None => e,
    })
}

fn inferred_data_kind(href: &str) -> content::DataKind {
    if content::is_glob_href(href) {
        content::DataKind::Glob
    } else {
        content::DataKind::from_path(Path::new(href)).unwrap_or(content::DataKind::Json)
    }
}

fn site_err(site: Option<(&str, &str)>, needles: &[&str], message: impl Into<String>) -> Error {
    match site {
        Some((file, source)) => Error::at(file, source, needles, message),
        None => Error::at_file("<unknown>", message),
    }
}

fn is_data_link(el: &Element) -> bool {
    matches!(el.statica_link_rel(), Some(StaticaLinkRel::Data))
}

/// Funnel `<link rel="statica/data" id="…">` ids declared on a page.
#[must_use]
pub fn data_link_ids(doc: &Document) -> Vec<String> {
    doc.find(is_data_link)
        .into_iter()
        .filter_map(|el| el.attr("id").map(str::trim).filter(|s| !s.is_empty()))
        .map(str::to_string)
        .collect()
}

/// Look up a field, distinguishing missing (`None`) from present `null` (`Some(Null)`).
/// Non-objects yield `None` (undefined) — only objects have enumerable own properties.
pub fn read_field<'a>(value: &'a Value, field: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map.get(field),
        _ => None,
    }
}

/// Render a bound value for attributes / text. `null` → empty string.
/// Objects and arrays are not stringified into attrs (empty); use slots for structure.
/// Returns `None` only for object/array (not valid attr scalars).
fn value_as_str(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some(String::new()),
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

pub fn field_as_str(value: &Value, field: &str) -> Option<String> {
    // Route / feed keys: missing and null are absent (not empty strings).
    match read_field(value, field) {
        None | Some(Value::Null) => None,
        Some(v) => value_as_str(v),
    }
}

/// Resolve a dotted path against a bind context object.
#[must_use]
pub fn path_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let mut cur = value;
    for part in path.split('.').filter(|p| !p.is_empty()) {
        cur = read_field(cur, part)?;
    }
    Some(cur)
}

/// Resolve `${path}` for attributes. Missing / null → empty (scope is checked statically).
pub fn path_as_str(value: &Value, path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return String::new();
    }
    match path_value(value, path) {
        None | Some(Value::Null) => String::new(),
        Some(v) => value_as_str(v).unwrap_or_default(),
    }
}

pub fn value_to_html(value: &Value) -> String {
    match value {
        Value::String(s) => {
            if s.contains('<') && s.contains('>') {
                s.clone()
            } else {
                escape_text(s)
            }
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null | Value::Array(_) | Value::Object(_) => String::new(),
    }
}

pub fn resolve_expr(
    expr: &str,
    current: Option<&Value>,
    local_data: &HashMap<String, DataSource>,
    parent_data: &HashMap<String, DataSource>,
) -> Result<Value> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Ok(Value::Null);
    }
    if expr == "." {
        return Ok(current.cloned().unwrap_or(Value::Null));
    }
    let mut parts = expr.split('.').filter(|p| !p.is_empty());
    let first = parts
        .next()
        .ok_or_else(|| Error::at_file("<data>", "empty data expression"))?;

    let mut value = if first == "this" {
        current.cloned().unwrap_or(Value::Null)
    } else if let Some(ds) = local_data.get(first) {
        ds.value()
    } else if let Some(ds) = parent_data.get(first) {
        ds.value()
    } else if let Some(cur) = current {
        match read_field(cur, first) {
            Some(v) => v.clone(),
            None => return Err(Error::at_file("<data>", missing_data_source_message(first))),
        }
    } else {
        return Err(Error::at_file("<data>", missing_data_source_message(first)));
    };

    for part in parts {
        value = match read_field(&value, part) {
            // missing → undefined → null in the funnel (renders empty)
            Some(v) => v.clone(),
            None => Value::Null,
        };
    }
    Ok(value)
}

fn content_cache_key(site_root: &Path, page_dir: &Path, src: &str) -> PathBuf {
    if Path::new(src).is_absolute() {
        normalize(&PathBuf::from(src))
    } else {
        normalize(&aliases::resolve_local_href(site_root, page_dir, src))
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Collect fragment link declarations from a document.
pub fn find_fragment_links(doc: &Document) -> Vec<(String, String)> {
    doc.find(|e| matches!(e.statica_link_rel(), Some(StaticaLinkRel::Fragment)))
        .into_iter()
        .filter_map(|el| Some((el.attr("id")?.to_string(), el.attr("href")?.to_string())))
        .collect()
}

/// Find `<template id=…>` element.
pub fn find_template<'a>(doc: &'a Document, id: &str) -> Option<&'a Element> {
    doc.find(|e| e.is_template() && e.attr("id") == Some(id))
        .into_iter()
        .next()
}

/// Strip statica authoring tags so output is browser-valid HTML.
pub fn strip_authoring(doc: &mut Document) {
    strip_nodes(&mut doc.children);
    for child in &mut doc.children {
        if let Node::Element(el) = child {
            if el.name.eq_ignore_ascii_case("html") {
                el.attrs.shift_remove("data-bind");
            }
        }
    }
}

fn strip_nodes(nodes: &mut Vec<Node>) {
    nodes.retain(|n| match n {
        Node::Element(el) => {
            if is_data_link(el) {
                return false;
            }
            if matches!(el.statica_link_rel(), Some(StaticaLinkRel::Fragment)) {
                return false;
            }
            true
        }
        _ => true,
    });
    for n in nodes.iter_mut() {
        if let Node::Element(el) = n {
            strip_nodes(&mut el.children);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn path_as_str_is_lenient_at_runtime() {
        let ctx = json!({"href": null, "variant": "primary"});
        assert_eq!(path_as_str(&ctx, "variant"), "primary");
        assert_eq!(path_as_str(&ctx, "href"), "");
        assert_eq!(path_as_str(&ctx, "missing"), "");
        assert_eq!(path_as_str(&json!({"obj": {"a": 1}}), "obj"), "");
    }

    #[test]
    fn value_reads_validate_js_types() {
        assert_eq!(value_as_str(&Value::Null).as_deref(), Some(""));
        assert_eq!(value_as_str(&json!("a")).as_deref(), Some("a"));
        assert_eq!(value_as_str(&json!(1)).as_deref(), Some("1"));
        assert_eq!(value_as_str(&json!(false)).as_deref(), Some("false"));
        assert!(value_as_str(&json!({})).is_none());
        assert!(value_as_str(&json!([])).is_none());
        assert_eq!(value_to_html(&Value::Null), "");
        assert_eq!(value_to_html(&json!({"a": 1})), "");
        // field_as_str keeps null/missing as absent for route keys
        assert!(field_as_str(&json!({"slug": null}), "slug").is_none());
        assert!(field_as_str(&json!({}), "slug").is_none());
        assert_eq!(
            field_as_str(&json!({"slug": "a"}), "slug").as_deref(),
            Some("a")
        );
    }
}
