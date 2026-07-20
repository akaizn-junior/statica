//! Template `data-bind` declarations and static bind-var scope checks.

use std::collections::HashSet;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingPattern, BindingProperty, ObjectPattern, PropertyKey, Statement,
};
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::loc;
use crate::parse::{Element, Node};

use super::json::read_field;

/// Source text used to attach `file:line:column` to bind diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct BindSource<'a> {
    pub file: &'a str,
    pub source: &'a str,
}

/// One binding from a destructure `data-bind` (leaf name + path into the value).
///
/// Flat `{variant}` → `name = "variant"`, `path = ["variant"]`.
/// Nested `{summary: { foo }}` → `name = "foo"`, `path = ["summary", "foo"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestructureBind {
    pub name: String,
    pub path: Vec<String>,
}

/// What a fragment `<template data-bind="…">` declares into scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindDecl {
    /// No `data-bind` — no bind names in scope.
    None,
    /// `data-bind="button"` — only `button` / `button.*` are in scope.
    Named(String),
    /// `data-bind="{variant, href}"` or nested `{tag, summary: { foo, bar }}`.
    /// Leaf binding names are in scope (taken from the bound object along `path`).
    Destructure(Vec<DestructureBind>),
}

impl BindDecl {
    #[must_use]
    pub fn scope_names(&self) -> HashSet<&str> {
        match self {
            Self::None => HashSet::new(),
            Self::Named(name) => HashSet::from([name.as_str()]),
            Self::Destructure(binds) => binds.iter().map(|b| b.name.as_str()).collect(),
        }
    }

    /// Flat destructure helper for tests / call sites: `{a, b}` → paths `[a]`, `[b]`.
    #[must_use]
    pub fn destructure_flat(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::Destructure(
            names
                .into_iter()
                .map(|n| {
                    let name = n.into();
                    DestructureBind {
                        path: vec![name.clone()],
                        name,
                    }
                })
                .collect(),
        )
    }
}

/// Parse a fragment template `data-bind` value.
///
/// Accepts a JS identifier (`button`) or object binding pattern
/// (`{variant, href}`, `{variant: variant}`, or nested `{summary: { foo, bar }}`).
/// Validated by oxc as a real binding pattern via `const <decl> = null;`.
pub fn parse_bind_decl(raw: Option<&str>) -> std::result::Result<BindDecl, String> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(BindDecl::None);
    };

    let wrapped = format!("const {raw} = null;");
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &wrapped, SourceType::mjs()).parse();
    if ret.panicked || !ret.diagnostics.is_empty() || ret.program.body.len() != 1 {
        return Err(format!(
            "data-bind=`{raw}` is not a JS identifier or destructure `{{a, b}}`"
        ));
    }

    let Statement::VariableDeclaration(decl) = &ret.program.body[0] else {
        return Err(format!(
            "data-bind=`{raw}` is not a JS identifier or destructure `{{a, b}}`"
        ));
    };
    if decl.declarations.len() != 1 {
        return Err(format!(
            "data-bind=`{raw}` is not a JS identifier or destructure `{{a, b}}`"
        ));
    }

    match &decl.declarations[0].id {
        BindingPattern::BindingIdentifier(id) => {
            Ok(BindDecl::Named(id.name.as_str().to_string()))
        }
        BindingPattern::ObjectPattern(obj) => {
            let mut binds = Vec::new();
            collect_object_pattern(raw, obj, &[], &mut binds)?;
            if binds.is_empty() {
                return Err("empty destructure `data-bind=\"{}\"`".into());
            }
            Ok(BindDecl::Destructure(binds))
        }
        BindingPattern::ArrayPattern(_) => Err(format!(
            "data-bind=`{raw}`: array destructure is not supported"
        )),
        BindingPattern::AssignmentPattern(_) => Err(format!(
            "data-bind=`{raw}`: default values are not supported"
        )),
    }
}

fn collect_object_pattern(
    raw: &str,
    obj: &ObjectPattern<'_>,
    path_prefix: &[String],
    out: &mut Vec<DestructureBind>,
) -> std::result::Result<(), String> {
    if obj.rest.is_some() {
        return Err(format!(
            "data-bind=`{raw}`: rest bindings are not supported"
        ));
    }
    if obj.properties.is_empty() {
        return Err(if path_prefix.is_empty() {
            "empty destructure `data-bind=\"{}\"`".into()
        } else {
            format!(
                "data-bind=`{raw}`: empty nested destructure for `{}`",
                path_prefix.last().unwrap()
            )
        });
    }
    for prop in &obj.properties {
        collect_binding_property(raw, prop, path_prefix, out)?;
    }
    Ok(())
}

fn collect_binding_property(
    raw: &str,
    prop: &BindingProperty<'_>,
    path_prefix: &[String],
    out: &mut Vec<DestructureBind>,
) -> std::result::Result<(), String> {
    if prop.computed {
        return Err(format!(
            "data-bind=`{raw}`: only plain identifier properties are supported"
        ));
    }
    let PropertyKey::StaticIdentifier(key) = &prop.key else {
        return Err(format!(
            "data-bind=`{raw}`: only plain identifier properties are supported"
        ));
    };
    let key_name = key.name.as_str();
    let mut path = path_prefix.to_vec();
    path.push(key_name.to_string());

    match &prop.value {
        BindingPattern::BindingIdentifier(id) => {
            let bind_name = id.name.as_str();
            if bind_name != key_name {
                return Err(format!(
                    "data-bind=`{raw}`: renames are not supported (`{key_name}: {bind_name}`)"
                ));
            }
            push_bind(
                raw,
                out,
                DestructureBind {
                    name: bind_name.to_string(),
                    path,
                },
            )
        }
        BindingPattern::ObjectPattern(nested) => {
            collect_object_pattern(raw, nested, &path, out)
        }
        BindingPattern::ArrayPattern(_) => Err(format!(
            "data-bind=`{raw}`: array destructure is not supported"
        )),
        BindingPattern::AssignmentPattern(_) => Err(format!(
            "data-bind=`{raw}`: default values are not supported"
        )),
    }
}

fn push_bind(
    raw: &str,
    out: &mut Vec<DestructureBind>,
    bind: DestructureBind,
) -> std::result::Result<(), String> {
    if out.iter().any(|b| b.name == bind.name) {
        return Err(format!(
            "data-bind=`{raw}`: duplicate name `{}`",
            bind.name
        ));
    }
    out.push(bind);
    Ok(())
}

fn read_path<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cur = value;
    for part in path {
        cur = read_field(cur, part)?;
    }
    Some(cur)
}

/// Build the runtime bind context from a declaration and the bound value.
///
/// - `Named("button")` → `{ "button": value }` only (no field flattening).
/// - `Destructure([…])` → pick each binding along its path from an object value.
/// - `None` → empty object.
pub fn bind_context(decl: &BindDecl, value: &Value) -> Value {
    match decl {
        BindDecl::None => Value::Object(serde_json::Map::new()),
        BindDecl::Named(name) => {
            let mut map = serde_json::Map::new();
            map.insert(name.clone(), value.clone());
            Value::Object(map)
        }
        BindDecl::Destructure(binds) => {
            let mut map = serde_json::Map::new();
            for bind in binds {
                let v = read_path(value, &bind.path)
                    .cloned()
                    .unwrap_or(Value::Null);
                map.insert(bind.name.clone(), v);
            }
            Value::Object(map)
        }
    }
}

/// Fail the build if `${…}` / named slots reference names not declared in `data-bind`.
pub fn validate_template_binds(
    fragment_id: &str,
    decl: &BindDecl,
    nodes: &[Node],
    source: BindSource<'_>,
) -> Result<()> {
    let scope = decl.scope_names();
    validate_nodes(fragment_id, &scope, nodes, source)
}

fn validate_nodes(
    fragment_id: &str,
    scope: &HashSet<&str>,
    nodes: &[Node],
    source: BindSource<'_>,
) -> Result<()> {
    for node in nodes {
        if let Node::Element(el) = node {
            validate_element(fragment_id, scope, el, source)?;
        }
    }
    Ok(())
}

fn validate_element(
    fragment_id: &str,
    scope: &HashSet<&str>,
    el: &Element,
    source: BindSource<'_>,
) -> Result<()> {
    if el.is_slot() && el.attr("id").is_none() {
        if let Some(name) = el.attr("name").map(str::trim).filter(|s| !s.is_empty()) {
            let dq = format!("name=\"{name}\"");
            let sq = format!("name='{name}'");
            ensure_bound(fragment_id, scope, name, name, source, &[&dq, &sq])?;
        }
    }
    if !el.is_script() && !el.is_style() {
        for (_k, v) in &el.attrs {
            if v.contains("${") {
                for path in template_paths(v) {
                    let root = path_root(&path);
                    let authored = format!("${{{path}}}");
                    ensure_bound(fragment_id, scope, &authored, root, source, &[&authored])?;
                }
            }
        }
    }
    validate_nodes(fragment_id, scope, &el.children, source)
}

fn ensure_bound(
    fragment_id: &str,
    scope: &HashSet<&str>,
    path: &str,
    root: &str,
    source: BindSource<'_>,
    needles: &[&str],
) -> Result<()> {
    if scope.contains(root) {
        return Ok(());
    }
    let (file, line, column, snippet) = loc::locate_any(source.file, source.source, needles);
    Err(Error::UnboundTemplateVar {
        file,
        line,
        column,
        id: fragment_id.to_string(),
        path: path.to_string(),
        name: root.to_string(),
        snippet,
    })
}

fn path_root(path: &str) -> &str {
    path.split('.').find(|p| !p.is_empty()).unwrap_or(path)
}

fn template_paths(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = raw[i + 2..].find('}') {
                let path = raw[i + 2..i + 2 + end].trim();
                if !path.is_empty() {
                    out.push(path.to_string());
                }
                i = i + 2 + end + 1;
                continue;
            }
        }
        i += raw[i..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use serde_json::json;

    fn el(name: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> Node {
        let mut map = IndexMap::new();
        for (k, v) in attrs {
            map.insert((*k).into(), (*v).into());
        }
        Node::Element(Element {
            name: name.into(),
            attrs: map,
            children,
            void: false,
        })
    }

    fn src(html: &str) -> BindSource<'_> {
        BindSource {
            file: "ui/button.html",
            source: html,
        }
    }

    #[test]
    fn parses_named_and_destructure() {
        assert_eq!(parse_bind_decl(None).unwrap(), BindDecl::None);
        assert_eq!(
            parse_bind_decl(Some("button")).unwrap(),
            BindDecl::Named("button".into())
        );
        assert_eq!(
            parse_bind_decl(Some("{variant, href}")).unwrap(),
            BindDecl::destructure_flat(["variant", "href"])
        );
        assert_eq!(
            parse_bind_decl(Some("{variant: variant, href: href}")).unwrap(),
            BindDecl::destructure_flat(["variant", "href"])
        );
        assert_eq!(
            parse_bind_decl(Some("{tag, title, summary: { foo, bar }}")).unwrap(),
            BindDecl::Destructure(vec![
                DestructureBind {
                    name: "tag".into(),
                    path: vec!["tag".into()],
                },
                DestructureBind {
                    name: "title".into(),
                    path: vec!["title".into()],
                },
                DestructureBind {
                    name: "foo".into(),
                    path: vec!["summary".into(), "foo".into()],
                },
                DestructureBind {
                    name: "bar".into(),
                    path: vec!["summary".into(), "bar".into()],
                },
            ])
        );
        assert!(parse_bind_decl(Some("button.variant")).is_err());
        assert!(parse_bind_decl(Some("{}")).is_err());
        assert!(parse_bind_decl(Some("{variant: other}")).is_err());
        assert!(parse_bind_decl(Some("{summary: {}}")).is_err());
        assert!(parse_bind_decl(Some("{foo, nested: { foo }}")).is_err());
    }

    #[test]
    fn named_bind_rejects_magic_fields() {
        let html = r#"<a class="button ${variant}" href="${href}"><slot name="label"></slot></a>"#;
        let decl = BindDecl::Named("button".into());
        let nodes = vec![el(
            "a",
            &[("class", "button ${variant}"), ("href", "${href}")],
            vec![el("slot", &[("name", "label")], vec![])],
        )];
        let err = validate_template_binds("button", &decl, &nodes, src(html)).unwrap_err();
        match err {
            Error::UnboundTemplateVar {
                ref file,
                line,
                column,
                ref name,
                ref snippet,
                ..
            } => {
                assert_eq!(name, "variant");
                assert_eq!(file, "ui/button.html");
                assert_eq!((line, column), (1, 18));
                assert!(snippet.contains("${variant}"));
                assert!(snippet.contains('^'));
            }
            other => panic!("unexpected: {other}"),
        }
    }

    #[test]
    fn named_bind_allows_prop_paths() {
        let html = r#"<a class="button ${button.variant}" href="${button.href}"></a>"#;
        let decl = BindDecl::Named("button".into());
        let nodes = vec![el(
            "a",
            &[("class", "button ${button.variant}"), ("href", "${button.href}")],
            vec![],
        )];
        validate_template_binds("button", &decl, &nodes, src(html)).unwrap();
    }

    #[test]
    fn destructure_allows_listed_names() {
        let html =
            r#"<a class="button ${variant}" href="${href}"><slot name="label"></slot></a>"#;
        let decl = BindDecl::destructure_flat(["variant", "href", "label"]);
        let nodes = vec![el(
            "a",
            &[("class", "button ${variant}"), ("href", "${href}")],
            vec![el("slot", &[("name", "label")], vec![])],
        )];
        validate_template_binds("button", &decl, &nodes, src(html)).unwrap();
    }

    #[test]
    fn named_slot_must_be_bound() {
        let html =
            r#"<a class="button ${variant}" href="${href}"><slot name="label"></slot></a>"#;
        let decl = BindDecl::destructure_flat(["variant", "href"]);
        let nodes = vec![el(
            "a",
            &[("class", "button ${variant}"), ("href", "${href}")],
            vec![el("slot", &[("name", "label")], vec![])],
        )];
        let err = validate_template_binds("button", &decl, &nodes, src(html)).unwrap_err();
        match err {
            Error::UnboundTemplateVar {
                line,
                column,
                ref name,
                ref snippet,
                ..
            } => {
                assert_eq!(name, "label");
                assert_eq!((line, column), (1, 51));
                assert!(snippet.contains("name=\"label\""));
            }
            other => panic!("unexpected: {other}"),
        }
    }

    #[test]
    fn bind_context_no_magic_flatten() {
        let button = json!({"variant": "primary", "href": "/go"});
        let ctx = bind_context(&BindDecl::Named("button".into()), &button);
        assert_eq!(ctx, json!({"button": button}));
        let destructured = bind_context(
            &BindDecl::destructure_flat(["variant", "href"]),
            &json!({"variant": "ghost", "href": "/x", "extra": 1}),
        );
        assert_eq!(destructured, json!({"variant": "ghost", "href": "/x"}));

        let nested = bind_context(
            &parse_bind_decl(Some("{tag, title, summary: { foo, bar }}")).unwrap(),
            &json!({
                "tag": "news",
                "title": "Hello",
                "summary": { "foo": 1, "bar": 2, "extra": 3 },
            }),
        );
        assert_eq!(
            nested,
            json!({"tag": "news", "title": "Hello", "foo": 1, "bar": 2})
        );
    }
}
