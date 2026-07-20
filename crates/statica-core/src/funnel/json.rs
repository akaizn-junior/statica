//! Local JSON funnel sources via `<script type="statica/data" src id>`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};
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

/// Opaque JSON payload — shape inferred by serde_json (TS-native style).
#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct JsonFile(Value);

pub fn load_data_from_document(
    doc: &Document,
    base_dir: &Path,
    cache: &mut HashMap<PathBuf, Value>,
) -> Result<HashMap<String, DataSource>> {
    let mut out = HashMap::new();
    for el in doc.find(|e| is_data_script(e)) {
        let id = el
            .attr("id")
            .ok_or_else(|| Error::msg("statica/data script missing id"))?
            .to_string();
        let src = el
            .attr("src")
            .ok_or_else(|| Error::msg(format!("statica/data#{id} missing src")))?;
        let path = resolve_path(base_dir, src)?;
        let value = if let Some(v) = cache.get(&path) {
            v.clone()
        } else {
            let text = fs::read_to_string(&path)
                .map_err(|e| Error::read(path.display().to_string(), e))?;
            let parsed: JsonFile = serde_json::from_str(&text)
                .map_err(|e| Error::invalid_json(path.display().to_string(), e))?;
            cache.insert(path.clone(), parsed.0.clone());
            parsed.0
        };
        out.insert(
            id.clone(),
            DataSource {
                id,
                path,
                value,
            },
        );
    }
    Ok(out)
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

/// Whether `name` is a valid JS IdentifierName (ASCII subset used for `data-bind` props).
#[must_use]
pub fn is_js_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c == '$' || c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '$' || c == '_' || c.is_ascii_alphanumeric())
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
        .ok_or_else(|| Error::msg("empty data expression"))?;

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
                return Err(Error::MissingData {
                    id: first.to_string(),
                })
            }
        }
    } else {
        return Err(Error::MissingData {
            id: first.to_string(),
        });
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

fn resolve_path(base_dir: &Path, rel: &str) -> Result<PathBuf> {
    let joined = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        base_dir.join(rel)
    };
    if let Ok(canon) = joined.canonicalize() {
        return Ok(canon);
    }
    let normalized = normalize(&joined);
    if normalized.exists() {
        return Ok(normalized);
    }
    if joined.exists() {
        return Ok(joined);
    }
    Err(Error::PathNotFound {
        path: joined.display().to_string(),
    })
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
    fn js_identifier_validation() {
        assert!(is_js_identifier("button"));
        assert!(is_js_identifier("_post"));
        assert!(is_js_identifier("$el"));
        assert!(is_js_identifier("a1"));
        assert!(!is_js_identifier(""));
        assert!(!is_js_identifier("1a"));
        assert!(!is_js_identifier("button.variant"));
        assert!(!is_js_identifier("post-card"));
    }

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
