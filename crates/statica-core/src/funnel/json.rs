//! Local JS funnel sources via `<script type="statica/data" src id>`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::aliases::{self, AliasOptions};
use crate::content;
use crate::error::{Error, Result};
use crate::i18n;
use crate::parse::escape_text;
use crate::parse::{Document, Element, Node};

use std::path::Component;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DataSource {
    pub id: String,
    pub path: PathBuf,
    pub value: Value,
}

pub fn document_has_locale_data(doc: &Document) -> bool {
    doc.find(|e| is_data_script(e)).into_iter().any(|el| {
        el.attr("src")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some_and(i18n::src_has_locale_token)
    })
}

/// Whether a specific funnel `<script type="statica/data" id="…">` uses `${locale}` in `src`.
pub fn data_script_has_locale_token(doc: &Document, id: &str) -> bool {
    doc.find(|e| is_data_script(e)).into_iter().any(|el| {
        el.attr("id") == Some(id)
            && el
                .attr("src")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some_and(i18n::src_has_locale_token)
    })
}

pub fn load_data_from_document(
    doc: &Document,
    base_dir: &Path,
    cache: &mut HashMap<PathBuf, Value>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<HashMap<String, DataSource>> {
    load_data_scripts(
        doc,
        base_dir,
        cache,
        aliases,
        site,
        DataScriptFilter::WithoutLocaleToken,
    )
}

/// Load funnel sources whose `src` contains `${locale}` for the active locale.
pub fn load_locale_data_from_document(
    doc: &Document,
    base_dir: &Path,
    cache: &mut HashMap<PathBuf, Value>,
    aliases: &AliasOptions,
    locale: &str,
    site: Option<(&str, &str)>,
) -> Result<HashMap<String, DataSource>> {
    load_data_scripts(
        doc,
        base_dir,
        cache,
        aliases,
        site,
        DataScriptFilter::WithLocaleTokenOnly { locale },
    )
}

enum DataScriptFilter<'a> {
    WithoutLocaleToken,
    WithLocaleTokenOnly { locale: &'a str },
}

fn load_data_scripts(
    doc: &Document,
    base_dir: &Path,
    cache: &mut HashMap<PathBuf, Value>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
    filter: DataScriptFilter<'_>,
) -> Result<HashMap<String, DataSource>> {
    let mut out = HashMap::new();
    for el in doc.find(|e| is_data_script(e)) {
        let id = match el.attr("id").map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => id.to_string(),
            None => {
                return Err(site_err(
                    site,
                    &["type=\"statica/data\"", "type='statica/data'"],
                    "statica/data script missing id",
                ));
            }
        };
        let src = match el.attr("src").map(str::trim).filter(|s| !s.is_empty()) {
            Some(src) => src,
            None => {
                let id_dq = format!("id=\"{id}\"");
                return Err(site_err(
                    site,
                    &["type=\"statica/data\"", id_dq.as_str()],
                    format!("statica/data#{id} missing src"),
                ));
            }
        };
        let src = aliases::resolve_path(src, aliases, site, "src")?;
        let has_locale_token = i18n::src_has_locale_token(&src);
        match filter {
            DataScriptFilter::WithoutLocaleToken if has_locale_token => continue,
            DataScriptFilter::WithLocaleTokenOnly { .. } if !has_locale_token => continue,
            _ => {}
        }
        let src = match filter {
            DataScriptFilter::WithLocaleTokenOnly { locale } => i18n::interpolate_locale(&src, locale),
            DataScriptFilter::WithoutLocaleToken => src,
        };
        let cache_key = content_cache_key(base_dir, &src);
        let value = if let Some(v) = cache.get(&cache_key) {
            v.clone()
        } else {
            let parsed = content::load_content(base_dir, &src).map_err(|e| match site {
                Some((file, source)) => {
                    let dq = format!("src=\"{src}\"");
                    let sq = format!("src='{src}'");
                    Error::at(file, source, &[&dq, &sq, src.as_str()], e.to_string())
                }
                None => e,
            })?;
            cache.insert(cache_key.clone(), parsed.clone());
            parsed
        };
        out.insert(
            id.clone(),
            DataSource {
                id,
                path: cache_key,
                value,
            },
        );
    }
    Ok(out)
}

fn site_err(
    site: Option<(&str, &str)>,
    needles: &[&str],
    message: impl Into<String>,
) -> Error {
    match site {
        Some((file, source)) => Error::at(file, source, needles, message),
        None => Error::at_file("<unknown>", message),
    }
}

fn is_data_script(el: &Element) -> bool {
    el.is_script() && el.attr("type").is_some_and(|t| t == "statica/data")
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

/// Resolve `${path}` for attributes. Missing / null → empty (scope is checked statically).
pub fn path_as_str(value: &Value, path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return String::new();
    }
    let mut cur = value;
    for part in path.split('.').filter(|p| !p.is_empty()) {
        match read_field(cur, part) {
            Some(next) => cur = next,
            None => return String::new(),
        }
    }
    match cur {
        Value::Null => String::new(),
        other => value_as_str(other).unwrap_or_default(),
    }
}

pub fn value_to_html(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => {
            if s.contains('<') && s.contains('>') {
                s.clone()
            } else {
                escape_text(s)
            }
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_) | Value::Object(_) => String::new(),
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
        ds.value.clone()
    } else if let Some(ds) = parent_data.get(first) {
        ds.value.clone()
    } else if let Some(cur) = current {
        match read_field(cur, first) {
            Some(v) => v.clone(),
            None => {
                return Err(Error::at_file(
                    "<data>",
                    format!(
                        "missing data source id `{first}` (no <script type=\"statica/data\" id=\"{first}\">)"
                    ),
                ))
            }
        }
    } else {
        return Err(Error::at_file(
            "<data>",
            format!(
                "missing data source id `{first}` (no <script type=\"statica/data\" id=\"{first}\">)"
            ),
        ));
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

fn content_cache_key(base_dir: &Path, src: &str) -> PathBuf {
    if Path::new(src).is_absolute() {
        normalize(&PathBuf::from(src))
    } else {
        normalize(&base_dir.join(src))
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
    doc.find(|e| {
        e.is_link()
            && e.attr("rel")
                .is_some_and(|r| r.split_whitespace().any(|p| p == "statica/fragment"))
    })
    .into_iter()
    .filter_map(|el| {
        Some((
            el.attr("id")?.to_string(),
            el.attr("href")?.to_string(),
        ))
    })
    .collect()
}

/// Find `<template id=…>` element.
pub fn find_template<'a>(doc: &'a Document, id: &str) -> Option<&'a Element> {
    doc.find(|e| e.is_template() && e.attr("id") == Some(id))
        .into_iter()
        .next()
}

/// Strip authoring tags according to [`crate::EmitOptions`].
pub fn strip_authoring(doc: &mut Document, opts: &crate::EmitOptions) {
    strip_nodes(&mut doc.children, opts);
    if opts.strip_html_data_bind {
        for child in &mut doc.children {
            if let Node::Element(el) = child {
                if el.name.eq_ignore_ascii_case("html") {
                    el.attrs.shift_remove("data-bind");
                }
            }
        }
    }
}

fn strip_nodes(nodes: &mut Vec<Node>, opts: &crate::EmitOptions) {
    nodes.retain(|n| match n {
        Node::Element(el) => {
            if is_data_script(el) {
                return !opts.strip_data;
            }
            if el.is_link()
                && el
                    .attr("rel")
                    .is_some_and(|r| r.split_whitespace().any(|p| p == "statica/fragment"))
            {
                return !opts.strip_fragments;
            }
            true
        }
        _ => true,
    });
    for n in nodes.iter_mut() {
        if let Node::Element(el) = n {
            strip_nodes(&mut el.children, opts);
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
        assert_eq!(field_as_str(&json!({"slug": "a"}), "slug").as_deref(), Some("a"));
    }
}
