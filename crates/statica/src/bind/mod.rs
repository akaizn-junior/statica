//! Bind funnel values into the AST (slots + attribute templates).

mod attrs;
mod slots;

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::discover::{PageKind, PageSource};
use crate::error::{Error, Result};
use crate::fragment::{self, FragmentRegistry};
use crate::funnel::{self, BindDecl, BindSource, DataSource};
use crate::i18n;
use crate::manifest::ManifestMeta;
use crate::parse::{Document, Element, Node};
use crate::scope;
use crate::{AliasOptions, FormsOptions};

pub(crate) use attrs::expand_template;
pub use attrs::fill_attr_templates_in_nodes;
pub use slots::{clear_remaining_named_slots, fill_default_slots, fill_named_slots};

fn html_element(doc: &Document) -> Option<&Element> {
    doc.children.iter().find_map(|n| match n {
        Node::Element(el) if el.name.eq_ignore_ascii_case("html") => Some(el),
        _ => None,
    })
}

/// Funnel id for collection/pagination routes.
///
/// From `data-bind="id"` on `<html>`, or the lone data link when using `data-bind="{…}"`.
#[must_use]
pub fn html_collection_id(doc: &Document) -> Option<String> {
    if let Some(raw) = html_bind_raw(doc) {
        if let Ok(BindDecl::Named(name)) = funnel::parse_bind_decl(Some(raw)) {
            return Some(name);
        }
    }
    let ids = funnel::data_link_ids(doc);
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
    let ids = funnel::data_link_ids(doc);
    let message = match ids.len() {
        0 => "collection page needs data-bind=\"id\" or data-bind=\"{…}\" with a data link",
        _ => "multiple data links — set data-bind=\"id\" on <html> to the collection id",
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
    [format!("data-bind=\"{id}\""), format!("data-bind='{id}'")]
}

fn parse_html_bind_decl(doc: &Document) -> Result<BindDecl> {
    let raw = html_bind_raw(doc);
    if raw.is_none() {
        return Ok(BindDecl::page_context());
    }
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
pub fn validate_page_binds(
    doc: &Document,
    source: BindSource<'_>,
    extra_roots: &[String],
) -> Result<()> {
    let Some(el) = html_element(doc) else {
        return Ok(());
    };
    let decl = parse_html_bind_decl(doc).map_err(|e| e.in_file(source.file, source.source))?;
    let mut data_roots = funnel::data_link_ids(doc);
    data_roots.extend(extra_roots.iter().cloned());
    funnel::validate_page_template_binds("page", &decl, &el.children, source, &data_roots)
}

/// Collection / pagination templates may use `<html data-bind>` to select a source id.
///
/// Locale-only routes (`[locale]/` with no other params) may bind `{locale}` without a data link.
pub fn validate_collection_page_binds(
    doc: &Document,
    kind: PageKind,
    locale_only: bool,
    source: BindSource<'_>,
    extra_roots: &[String],
) -> Result<()> {
    if kind != PageKind::Collection {
        return Ok(());
    }
    if html_bind_raw(doc).is_none() {
        return Ok(());
    }
    if !(locale_only && funnel::data_link_ids(doc).is_empty()) {
        require_collection_id(doc, source)?;
    }
    validate_page_binds(doc, source, extra_roots)
}

/// Render a full page document with optional item context.
pub fn render_page_document(
    registry: &FragmentRegistry,
    doc: &Document,
    source: &PageSource,
    current: Option<&Value>,
    page_data: &HashMap<String, DataSource>,
    aliases: &AliasOptions,
    forms: &FormsOptions,
    manifest: Option<&ManifestMeta>,
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, crate::content::DataSet>,
    site: Option<(&str, &str)>,
) -> Result<String> {
    let mut doc = doc.clone();
    let page_context = build_page_context(source, current, page_data, locale);
    let bind = html_element(&doc)
        .and_then(|el| el.attr("data-bind"))
        .and_then(|raw| funnel::parse_bind_decl(Some(raw)).ok())
        .unwrap_or_else(|| BindDecl::Named("ctx".into()));
    let mut ctx = match bind {
        BindDecl::Named(ref name) if name == "ctx" => page_context.clone(),
        _ => funnel::bind_context(&bind, &page_context),
    };
    add_data_roots(&mut ctx, page_data);
    fill_attr_templates_in_nodes(&mut doc.children, &ctx);
    fill_named_slots(&mut doc.children, &ctx);
    let context_data = context_data_sources(page_data, &page_context);
    expand_usage_slots_in_nodes(
        registry,
        &mut doc.children,
        current,
        &context_data,
        locale,
        i18n_catalog,
        data_cache,
        aliases,
        site,
    )?;
    let data_t_context = data_t_context(&ctx, i18n_catalog);
    i18n::apply_data_t(&mut doc.children, &data_t_context);
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

fn add_data_roots(ctx: &mut Value, page_data: &HashMap<String, DataSource>) {
    let Value::Object(map) = ctx else {
        return;
    };
    for (id, source) in page_data {
        map.entry(id.clone()).or_insert_with(|| source.value());
    }
}

fn data_t_context(ctx: &Value, i18n_catalog: Option<&Value>) -> Value {
    let Some(catalog) = i18n_catalog else {
        return ctx.clone();
    };
    let mut merged = match ctx {
        Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    let i18n = merged
        .get("i18n")
        .map_or_else(|| catalog.clone(), |base| deep_merge(base, catalog));
    merged.insert("i18n".into(), i18n);
    Value::Object(merged)
}

fn deep_merge(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            let mut out = base_map.clone();
            for (key, value) in overlay_map {
                out.insert(
                    key.clone(),
                    match out.get(key) {
                        Some(existing) if existing.is_object() && value.is_object() => {
                            deep_merge(existing, value)
                        }
                        _ => value.clone(),
                    },
                );
            }
            Value::Object(out)
        }
        (_, overlay) => overlay.clone(),
    }
}

fn context_data_sources(
    page_data: &HashMap<String, DataSource>,
    page_context: &Value,
) -> HashMap<String, DataSource> {
    let mut out = page_data.clone();
    for id in funnel::CANONICAL_PAGE_ROOTS {
        let value = funnel::read_field(page_context, id)
            .cloned()
            .unwrap_or(Value::Null);
        out.insert(
            (*id).to_string(),
            DataSource {
                id: (*id).to_string(),
                kind: crate::content::DataKind::Json,
                path: PathBuf::from(format!("statica:{id}")),
                data: crate::content::DataSet::Json(value),
            },
        );
    }
    out
}

fn build_page_context(
    source: &PageSource,
    current: Option<&Value>,
    page_data: &HashMap<String, DataSource>,
    locale: Option<&str>,
) -> Value {
    let mut data = serde_json::Map::new();
    for (id, source) in page_data {
        data.insert(id.clone(), source.value());
    }

    let current = current.cloned().unwrap_or(Value::Null);
    let is_pagination = current
        .as_object()
        .is_some_and(|obj| obj.contains_key("items") && obj.contains_key("total_pages"));

    let mut params = serde_json::Map::new();
    for param in &source.params {
        let value = if param == i18n::LOCALE_PARAM {
            locale.map_or(Value::Null, |loc| Value::String(loc.to_string()))
        } else {
            funnel::read_field(&current, param)
                .cloned()
                .unwrap_or(Value::Null)
        };
        params.insert(param.clone(), value);
    }

    let mut page = serde_json::Map::new();
    page.insert("route".into(), Value::String(source.route.clone()));
    page.insert("params".into(), Value::Object(params));
    if is_pagination {
        page.insert("pagination".into(), current.clone());
    }

    serde_json::json!({
        "data": data,
        "item": if is_pagination { Value::Null } else { current },
        "page": page,
        "i18n": {
            "locale": locale.unwrap_or("")
        }
    })
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
    data_cache: &mut HashMap<PathBuf, crate::content::DataSet>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<()> {
    let mut i = 0;
    while i < nodes.len() {
        let replace = match &nodes[i] {
            Node::Element(el)
                if el.is_slot() && el.attr("id").is_some() && el.attr("name").is_none() =>
            {
                let id = el.attr("id").unwrap_or("").to_string();
                let children_html_nodes = el.children.clone();
                let each = el.attr("data-each").map(str::to_string);
                Some((id, children_html_nodes, each))
            }
            _ => None,
        };

        if let Some((id, children_nodes, each)) = replace {
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
                let value = current.cloned().unwrap_or(Value::Null);
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
    data_cache: &mut HashMap<PathBuf, crate::content::DataSet>,
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
    data_cache: &mut HashMap<PathBuf, crate::content::DataSet>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<Vec<Node>> {
    let frag = registry.get(id).ok_or_else(|| {
        let msg =
            format!("missing fragment id `{id}` (no <link rel=\"statica/fragment\" id=\"{id}\">)");
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
    let data_t_context = data_t_context(&ctx, i18n_catalog);
    i18n::apply_data_t(&mut nodes, &data_t_context);
    Ok(nodes)
}
