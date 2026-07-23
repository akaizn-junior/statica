//! Bind funnel values into the AST (slots + attribute templates).

mod attrs;
mod slots;

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::fragment::{self, FragmentRegistry};
use crate::discover::PageKind;
use crate::funnel::{self, BindDecl, BindSource, DataSource};
use crate::parse::{Document, Element, Node};
use crate::scope;
use crate::{AliasOptions, FormsOptions};
use crate::manifest::ManifestMeta;
use crate::i18n;

pub use attrs::fill_attr_templates_in_nodes;
pub use slots::{clear_remaining_named_slots, fill_default_slots, fill_named_slots};

fn html_element<'a>(doc: &'a Document) -> Option<&'a Element> {
    doc.children.iter().find_map(|n| match n {
        Node::Element(el) if el.name.eq_ignore_ascii_case("html") => Some(el),
        _ => None,
    })
}

/// Funnel id for collection/pagination routes.
///
/// From `data-bind="id"` on `<html>`, or the lone funnel script when using `data-bind="{…}"`.
#[must_use]
pub fn html_collection_id(doc: &Document) -> Option<String> {
    if let Some(raw) = html_bind_raw(doc) {
        if let Ok(BindDecl::Named(name)) = funnel::parse_bind_decl(Some(raw)) {
            return Some(name);
        }
    }
    let ids = funnel::data_script_ids(doc);
    if ids.len() == 1 {
        return Some(ids[0].clone());
    }
    None
}

/// Resolve the funnel id for a collection/pagination template.
pub fn require_collection_id(doc: &Document, source: BindSource<'_>) -> Result<String> {
    if let Some(id) = html_collection_id(doc) {
        return Ok(id);
    }
    let ids = funnel::data_script_ids(doc);
    let message = match ids.len() {
        0 => "collection page needs data-bind=\"id\" or data-bind=\"{…}\" with a funnel script",
        _ => {
            "multiple funnel scripts — set data-bind=\"id\" on <html> to the collection id"
        }
    };
    Err(Error::at(
        source.file,
        source.source,
        &["<html", "data-bind"],
        message,
    ))
}

fn html_bind_raw(doc: &Document) -> Option<&str> {
    html_element(doc).and_then(|el| el.attr("data-bind"))
}

pub fn collection_needles(id: &str) -> [String; 2] {
    [
        format!("data-bind=\"{id}\""),
        format!("data-bind='{id}'"),
    ]
}

/// Whether the page declares a `<html data-bind>` scope.
#[must_use]
pub fn html_has_bind(doc: &Document) -> bool {
    html_bind_raw(doc).is_some()
}

fn parse_html_bind_decl(doc: &Document) -> Result<BindDecl> {
    let raw = html_bind_raw(doc);
    funnel::parse_bind_decl(raw).map_err(|reason| {
        let prop = raw.unwrap_or("");
        Error::at(
            "<page>",
            "",
            &[
                &format!("data-bind=\"{prop}\""),
                &format!("data-bind='{prop}'"),
            ],
            reason,
        )
    })
}

/// Fail the build if page slots / `${…}` / mount binds reference names not in `<html data-bind>`.
pub fn validate_page_binds(doc: &Document, source: BindSource<'_>) -> Result<()> {
    let Some(el) = html_element(doc) else {
        return Ok(());
    };
    let decl = parse_html_bind_decl(doc).map_err(|e| e.in_file(source.file, source.source))?;
    funnel::validate_page_template_binds("page", &decl, &el.children, source)
}

/// Collection / pagination templates must declare `<html data-bind>`.
///
/// Locale-only routes (`[locale]/` with no other params) may bind `{locale}` without a funnel script.
pub fn validate_collection_page_binds(
    doc: &Document,
    kind: PageKind,
    locale_only: bool,
    source: BindSource<'_>,
) -> Result<()> {
    if kind != PageKind::Collection {
        return Ok(());
    }
    if html_bind_raw(doc).is_none() {
        return Ok(());
    }
    if !(locale_only && funnel::data_script_ids(doc).is_empty()) {
        require_collection_id(doc, source)?;
    }
    validate_page_binds(doc, source)
}

/// Render a full page document with optional item context.
pub fn render_page_document(
    registry: &FragmentRegistry,
    doc: &Document,
    current: Option<&Value>,
    page_data: &HashMap<String, DataSource>,
    aliases: &AliasOptions,
    forms: &FormsOptions,
    manifest: Option<&ManifestMeta>,
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, Value>,
    site: Option<(&str, &str)>,
) -> Result<String> {
    let mut doc = doc.clone();
    if let Some(current) = current {
        let bind = html_element(&doc)
            .and_then(|el| funnel::parse_bind_decl(el.attr("data-bind")).ok())
            .unwrap_or(BindDecl::None);
        let ctx = funnel::bind_context(&bind, current);
        fill_attr_templates_in_nodes(&mut doc.children, &ctx);
        fill_named_slots(&mut doc.children, &ctx);
    }
    expand_usage_slots_in_nodes(
        registry,
        &mut doc.children,
        current,
        page_data,
        locale,
        i18n_catalog,
        data_cache,
        aliases,
        site,
    )?;
    if let Some(catalog) = i18n_catalog {
        i18n::apply_data_t(&mut doc.children, catalog);
    } else {
        i18n::strip_data_t(&mut doc.children);
    }
    crate::aliases::resolve_paths_in_document(&mut doc, aliases, site)?;
    crate::font::expand_font_links(&mut doc, aliases, site)?;
    if let Some(meta) = manifest {
        crate::manifest::inject_manifest_tags(&mut doc, meta);
    }
    crate::forms::wire_forms_in_document(&mut doc, forms, site)?;
    funnel::strip_authoring(&mut doc);
    clear_remaining_named_slots(&mut doc.children);
    transform_page_styles(&mut doc.children);
    scope::dedupe_helpers_in_document(&mut doc);
    scope::dedupe_styles_in_document(&mut doc);
    Ok(crate::parse::serialize_document(&doc))
}

/// Transform unscoped page `<style>` (fragment styles already went through
/// [`crate::css::transform_and_scope`]).
fn transform_page_styles(nodes: &mut [Node]) {
    for node in nodes {
        if let Node::Element(el) = node {
            if el.is_style() {
                if let Some(Node::Text(css)) = el.children.first_mut() {
                    // Fragment styles already contain [data-s="…"] after scoping.
                    if !css.contains("[data-s=\"") {
                        if let Ok(ready) = crate::css::transform_css(css, true) {
                            *css = ready;
                        }
                    }
                }
            }
            transform_page_styles(&mut el.children);
        }
    }
}

/// Expand `<slot id>` mounts (and `data-each` loops) in-place.
pub fn expand_usage_slots_in_nodes(
    registry: &FragmentRegistry,
    nodes: &mut Vec<Node>,
    current: Option<&Value>,
    data_map: &HashMap<String, DataSource>,
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, Value>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<()> {
    let mut i = 0;
    while i < nodes.len() {
        let replace = match &nodes[i] {
            Node::Element(el) if el.is_slot() && el.attr("id").is_some() && el.attr("name").is_none() => {
                let id = el.attr("id").unwrap_or("").to_string();
                let children_html_nodes = el.children.clone();
                let each = el.attr("data-each").map(str::to_string);
                let bind = el.attr("data-bind").map(str::to_string);
                Some((id, children_html_nodes, each, bind))
            }
            _ => None,
        };

        if let Some((id, children_nodes, each, bind)) = replace {
            let rendered = if let Some(each_expr) = each {
                let list = funnel::resolve_expr(&each_expr, current, data_map, data_map)
                    .map_err(|e| relocate_data_err(e, site, &each_expr))?;
                render_each(
                    registry,
                    &id,
                    &list,
                    data_map,
                    &children_nodes,
                    locale,
                    i18n_catalog,
                    data_cache,
                    aliases,
                    site,
                    &each_expr,
                )?
            } else {
                let value = match bind.as_deref() {
                    None | Some("") => Value::Null,
                    Some(b) => funnel::resolve_expr(b, current, data_map, data_map)
                        .map_err(|e| relocate_data_err(e, site, b))?,
                };
                render_fragment_nodes(
                    registry,
                    &id,
                    &value,
                    data_map,
                    &children_nodes,
                    locale,
                    i18n_catalog,
                    data_cache,
                    aliases,
                    site,
                )?
            };
            nodes.splice(i..=i, rendered.iter().cloned());
            i += rendered.len().max(1);
            continue;
        }

        if let Node::Element(el) = &mut nodes[i] {
            expand_usage_slots_in_nodes(
                registry,
                &mut el.children,
                current,
                data_map,
                locale,
                i18n_catalog,
                data_cache,
                aliases,
                site,
            )?;
        }
        i += 1;
    }
    Ok(())
}

fn relocate_data_err(err: Error, site: Option<(&str, &str)>, expr: &str) -> Error {
    match site {
        Some((file, source)) => {
            let dq = format!("data-bind=\"{expr}\"");
            let sq = format!("data-bind='{expr}'");
            let each_dq = format!("data-each=\"{expr}\"");
            let each_sq = format!("data-each='{expr}'");
            err.in_file_at(file, source, &[&dq, &sq, &each_dq, &each_sq, expr])
        }
        None => err,
    }
}

fn render_each(
    registry: &FragmentRegistry,
    id: &str,
    list: &Value,
    data_map: &HashMap<String, DataSource>,
    children: &[Node],
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, Value>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
    each_expr: &str,
) -> Result<Vec<Node>> {
    let arr = match list {
        Value::Array(a) => a,
        Value::Null => return Ok(Vec::new()),
        _ => {
            let msg = format!("data-each for `{id}` expected an array");
            return Err(match site {
                Some((file, source)) => {
                    let dq = format!("data-each=\"{each_expr}\"");
                    let sq = format!("data-each='{each_expr}'");
                    Error::at(file, source, &[&dq, &sq], msg)
                }
                None => Error::at_file("<page>", msg),
            });
        }
    };
    let mut out = Vec::new();
    for item in arr {
        out.extend(render_fragment_nodes(
            registry,
            id,
            item,
            data_map,
            children,
            locale,
            i18n_catalog,
            data_cache,
            aliases,
            site,
        )?);
    }
    Ok(out)
}

fn render_fragment_nodes(
    registry: &FragmentRegistry,
    id: &str,
    prop_value: &Value,
    parent_data: &HashMap<String, DataSource>,
    children: &[Node],
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, Value>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<Vec<Node>> {
    let frag = registry.get(id).ok_or_else(|| {
        let msg = format!(
            "missing fragment id `{id}` (no <link rel=\"statica/fragment\" id=\"{id}\">)"
        );
        match site {
            Some((file, source)) => {
                let dq = format!("id=\"{id}\"");
                let sq = format!("id='{id}'");
                Error::at(file, source, &[&dq, &sq], msg)
            }
            None => Error::at_file("<page>", msg),
        }
    })?;

    let frag_data = registry.resolve_fragment_data(frag, locale, data_cache, aliases)?;
    let mut local = parent_data.clone();
    for (k, v) in &frag_data {
        local.insert(k.clone(), v.clone());
    }

    // `data-bind="button"` → only `button` in scope; `data-bind="{a,b}"` → those fields.
    let ctx = funnel::bind_context(&frag.bind, prop_value);

    let mut nodes = fragment::template_children(frag);
    scope::apply_scope_to_nodes(&mut nodes, &frag.scope_id);
    fill_attr_templates_in_nodes(&mut nodes, &ctx);
    fill_named_slots(&mut nodes, &ctx);
    fill_default_slots(&mut nodes, children);
    scope::rewrite_scripts_in_nodes(&mut nodes, &frag.scope_id);
    expand_usage_slots_in_nodes(
        registry,
        &mut nodes,
        Some(prop_value),
        &local,
        locale,
        i18n_catalog,
        data_cache,
        aliases,
        site,
    )?;
    if let Some(catalog) = i18n_catalog {
        i18n::apply_data_t(&mut nodes, catalog);
    } else {
        i18n::strip_data_t(&mut nodes);
    }
    Ok(nodes)
}
