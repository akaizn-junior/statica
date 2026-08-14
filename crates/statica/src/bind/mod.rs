//! Bind funnel values into the AST (slots + attribute templates).

mod attrs;
mod slots;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::context::{CanonicalContext, ContextData, ContextScope, ContextTree};
use crate::discover::{PageKind, PageSource};
use crate::error::{Error, Result};
use crate::fragment::{self, FragmentRegistry};
use crate::funnel::{self, BindDecl, BindSource, DataSource};
use crate::i18n;
use crate::manifest::ManifestMeta;
use crate::parse::{Document, EachDirective, Element, Node, SlotKind};
use crate::render::{Op, PageRenderer, RenderPlan};
use crate::scope;
use crate::tokens::missing_fragment_message;
use crate::{AliasOptions, FormsOptions};

pub(crate) use attrs::expand_template;
pub use attrs::fill_attr_templates_in_nodes;
pub use slots::{clear_remaining_named_slots, fill_default_slots, fill_named_slots};

#[derive(Debug, Clone)]
struct FragmentMount {
    id: String,
    children: Vec<Node>,
    each: Option<EachDirective>,
}

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
    html_element(doc).and_then(Element::bind_directive)
}

pub fn collection_needles(id: &str) -> [String; 2] {
    [format!("data-bind=\"{id}\""), format!("data-bind='{id}'")]
}

fn parse_html_bind_decl(doc: &Document) -> Result<BindDecl> {
    let raw = html_bind_raw(doc);
    if raw.is_none() {
        return Ok(BindDecl::None);
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
pub fn validate_collection_page_binds(
    doc: &Document,
    kind: PageKind,
    has_collection_param: bool,
    source: BindSource<'_>,
    extra_roots: &[String],
) -> Result<()> {
    if kind != PageKind::Collection {
        return Ok(());
    }
    if html_bind_raw(doc).is_none() {
        return Ok(());
    }
    if !has_collection_param {
        return validate_page_binds(doc, source, extra_roots);
    }
    require_collection_id(doc, source)?;
    validate_page_binds(doc, source, extra_roots)
}

/// Render a full page document with optional item context.
pub fn render_page_document(
    registry: &FragmentRegistry,
    doc: &Document,
    render_plan: &RenderPlan,
    source: &PageSource,
    current: Option<&Value>,
    page_data: &HashMap<String, DataSource>,
    aliases: &AliasOptions,
    _forms: &FormsOptions,
    _manifest: Option<&ManifestMeta>,
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    site: Option<(&str, &str)>,
) -> Result<String> {
    let bind = html_element(doc)
        .and_then(Element::bind_directive)
        .and_then(|raw| funnel::parse_bind_decl(Some(raw)).ok())
        .unwrap_or(BindDecl::None);
    let canonical = CanonicalContext::new(source, current, page_data, locale, &bind.scope_names());
    let bind_ctx = funnel::bind_context(&bind, canonical.value());
    let context_data = canonical.as_data_sources(page_data);
    let context_tree = ContextTree::new(ContextScope::Page, bind_ctx, context_data.clone());

    match PageRenderer::select(registry, doc, locale) {
        PageRenderer::CompiledPlan => render_with_compiled_plan(
            registry,
            render_plan,
            current,
            context_data.as_map(),
            &context_tree.render_context_with_linked_roots(Some(render_plan.linked_roots())),
            &context_tree.translated_context_with_linked_roots(
                i18n_catalog,
                Some(render_plan.linked_roots()),
            ),
            locale,
            i18n_catalog,
            data_cache,
            aliases,
            site,
        ),
        PageRenderer::AstMutation => render_with_ast_mutation(
            registry,
            doc,
            current,
            context_data.as_map(),
            &context_tree,
            locale,
            i18n_catalog,
            data_cache,
            aliases,
            site,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_with_ast_mutation(
    registry: &FragmentRegistry,
    doc: &Document,
    current: Option<&Value>,
    data_map: &HashMap<String, DataSource>,
    context_tree: &ContextTree,
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<String> {
    let mut doc = doc.clone();
    let ctx = context_tree.render_context();
    fill_attr_templates_in_nodes(&mut doc.children, &ctx);
    fill_named_slots(&mut doc.children, &ctx);
    expand_usage_slots_in_nodes(
        registry,
        &mut doc.children,
        current,
        data_map,
        locale,
        i18n_catalog,
        data_cache,
        aliases,
        site,
    )?;
    let data_t_context = context_tree.translated_context(i18n_catalog);
    i18n::apply_data_t(&mut doc.children, &data_t_context);
    funnel::strip_authoring(&mut doc);
    clear_remaining_named_slots(&mut doc.children);
    scope::dedupe_helpers_in_document(&mut doc);
    scope::dedupe_styles_in_document(&mut doc);
    Ok(crate::parse::serialize_document(&doc))
}

#[allow(clippy::too_many_arguments)]
fn render_with_compiled_plan(
    registry: &FragmentRegistry,
    render_plan: &RenderPlan,
    current: Option<&Value>,
    data_map: &HashMap<String, DataSource>,
    attr_context: &Value,
    text_context: &Value,
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<String> {
    let mut out = String::with_capacity(4096);
    render_plan.write_doctype(&mut out);
    render_plan_ops(
        registry,
        render_plan.ops(),
        current,
        data_map,
        attr_context,
        text_context,
        locale,
        i18n_catalog,
        data_cache,
        aliases,
        site,
        &[],
        &mut out,
    )?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn render_plan_ops(
    registry: &FragmentRegistry,
    ops: &[Op],
    current: Option<&Value>,
    data_map: &HashMap<String, DataSource>,
    attr_context: &Value,
    text_context: &Value,
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
    default_children: &[Op],
    out: &mut String,
) -> Result<()> {
    for op in ops {
        match op {
            Op::Static(text) => out.push_str(text),
            Op::AttrTemplate(template) => {
                out.push_str(&escape_attr(&expand_template(template, attr_context)));
            }
            Op::TextTemplate(template) => out.push_str(&expand_template(template, text_context)),
            Op::NamedSlot(name) => {
                if let Some(value) = funnel::path_value(attr_context, name) {
                    out.push_str(&funnel::value_to_html(value));
                }
            }
            Op::DefaultSlot(fallback) => {
                let children = if default_children.is_empty() {
                    fallback.as_slice()
                } else {
                    default_children
                };
                render_plan_ops(
                    registry,
                    children,
                    current,
                    data_map,
                    attr_context,
                    text_context,
                    locale,
                    i18n_catalog,
                    data_cache,
                    aliases,
                    site,
                    default_children,
                    out,
                )?;
            }
            Op::Mount { id, each, children } => {
                render_plan_mount(
                    registry,
                    id,
                    each.as_deref(),
                    children,
                    current,
                    data_map,
                    locale,
                    i18n_catalog,
                    data_cache,
                    aliases,
                    site,
                    out,
                )?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_plan_mount(
    registry: &FragmentRegistry,
    id: &str,
    each: Option<&str>,
    children: &[Op],
    current: Option<&Value>,
    data_map: &HashMap<String, DataSource>,
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
    out: &mut String,
) -> Result<()> {
    if let Some(each_expr) = each {
        let list = resolve_each_array(each_expr, current, data_map, data_map)
            .map_err(|e| relocate_data_err(e, site, each_expr))?;
        match list {
            Some(items) => {
                for item in items.iter() {
                    render_plan_fragment(
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
                        out,
                    )?;
                }
                Ok(())
            }
            None => Ok(()),
        }
    } else {
        let value = current.cloned().unwrap_or(Value::Null);
        render_plan_fragment(
            registry,
            id,
            &value,
            data_map,
            children,
            locale,
            i18n_catalog,
            data_cache,
            aliases,
            site,
            out,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn render_plan_fragment(
    registry: &FragmentRegistry,
    id: &str,
    prop_value: &Value,
    parent_data: &HashMap<String, DataSource>,
    children: &[Op],
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
    out: &mut String,
) -> Result<()> {
    let frag = registry.get(id).ok_or_else(|| {
        let msg = missing_fragment_message(id);
        match site {
            Some((file, source)) => {
                let dq = format!("id=\"{id}\"");
                let sq = format!("id='{id}'");
                Error::at(file, source, &[&dq, &sq], msg)
            }
            None => Error::at_file("<page>", msg),
        }
    })?;
    let frag_data =
        registry.resolve_fragment_data(frag, Some(prop_value), locale, data_cache, aliases)?;
    let local = ContextData::new(parent_data.clone()).with_links(&frag_data);
    let bind_ctx = funnel::bind_context(&frag.bind, prop_value);
    let context_tree = ContextTree::new(ContextScope::Fragment, bind_ctx, local.clone());
    render_plan_ops(
        registry,
        frag.render_plan.ops(),
        Some(prop_value),
        local.as_map(),
        &context_tree.render_context_with_linked_roots(Some(frag.render_plan.linked_roots())),
        &context_tree.translated_context_with_linked_roots(
            i18n_catalog,
            Some(frag.render_plan.linked_roots()),
        ),
        locale,
        i18n_catalog,
        data_cache,
        aliases,
        site,
        children,
        out,
    )
}

fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Transform unscoped page `<style>` (fragment styles already went through
/// [`crate::css::transform_and_scope`]).
pub fn transform_page_styles(nodes: &mut [Node]) {
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
    data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<()> {
    let mut i = 0;
    while i < nodes.len() {
        let replace = match &nodes[i] {
            Node::Element(el) => match el.slot_kind() {
                Some(SlotKind::FragmentMount(id)) => Some(FragmentMount {
                    id,
                    children: el.children.clone(),
                    each: el.each_directive(),
                }),
                Some(SlotKind::Named(_) | SlotKind::Default) | None => None,
            },
            _ => None,
        };

        if let Some(mount) = replace {
            let rendered = if let Some(each) = mount.each {
                let list = resolve_each_array(each.expr(), current, data_map, data_map)
                    .map_err(|e| relocate_data_err(e, site, each.expr()))?;
                render_each(
                    registry,
                    &mount.id,
                    list,
                    data_map,
                    &mount.children,
                    locale,
                    i18n_catalog,
                    data_cache,
                    aliases,
                    site,
                    each.expr(),
                )?
            } else {
                let value = current.cloned().unwrap_or(Value::Null);
                render_fragment_nodes(
                    registry,
                    &mount.id,
                    &value,
                    data_map,
                    &mount.children,
                    locale,
                    i18n_catalog,
                    data_cache,
                    aliases,
                    site,
                )?
            };
            nodes.splice(i..=i, rendered.iter().cloned());
            i += rendered.len().max(1);
        } else {
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

fn resolve_each_array<'a>(
    expr: &str,
    current: Option<&'a Value>,
    local_data: &'a HashMap<String, DataSource>,
    parent_data: &'a HashMap<String, DataSource>,
) -> Result<Option<Cow<'a, [Value]>>> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Ok(None);
    }
    if expr == "." {
        return match current {
            Some(Value::Array(items)) => Ok(Some(Cow::Borrowed(items))),
            Some(Value::Null) | None => Ok(None),
            Some(_) => Err(Error::at_file("<data>", "data-each expected an array")),
        };
    }

    let mut parts = expr.split('.').filter(|p| !p.is_empty());
    let first = parts
        .next()
        .ok_or_else(|| Error::at_file("<data>", "empty data expression"))?;
    let rest: Vec<&str> = parts.collect();

    if first == "this" {
        return array_at_path(current.unwrap_or(&Value::Null), &rest);
    }
    if let Some(value) = current.and_then(|cur| funnel::read_field(cur, first)) {
        if let Some(items) = array_at_path(value, &rest)? {
            return Ok(Some(items));
        }
    }
    if let Some(source) = local_data.get(first).or_else(|| parent_data.get(first)) {
        if let Some(value) = data_source_value(source) {
            return array_at_path(value, &rest);
        }
        if rest.is_empty() {
            return source
                .array()
                .map(|items| Some(Cow::Owned(items)))
                .ok_or_else(|| Error::at_file("<data>", "data-each expected an array"));
        }
    }

    let value = funnel::resolve_expr(expr, current, local_data, parent_data)?;
    match value {
        Value::Array(items) => Ok(Some(Cow::Owned(items))),
        Value::Null => Ok(None),
        _ => Err(Error::at_file("<data>", "data-each expected an array")),
    }
}

fn array_at_path<'a>(mut value: &'a Value, path: &[&str]) -> Result<Option<Cow<'a, [Value]>>> {
    for part in path {
        value = match funnel::read_field(value, part) {
            Some(value) => value,
            None => return Ok(None),
        };
    }
    match value {
        Value::Array(items) => Ok(Some(Cow::Borrowed(items))),
        Value::Null => Ok(None),
        _ => Err(Error::at_file("<data>", "data-each expected an array")),
    }
}

fn data_source_value(source: &DataSource) -> Option<&Value> {
    match source.data.as_ref() {
        crate::content::DataSet::Json(value) | crate::content::DataSet::Markdown(value) => {
            Some(value)
        }
        crate::content::DataSet::Records(_)
        | crate::content::DataSet::Lines(_)
        | crate::content::DataSet::Glob(_) => None,
    }
}

fn render_each(
    registry: &FragmentRegistry,
    id: &str,
    list: Option<Cow<'_, [Value]>>,
    data_map: &HashMap<String, DataSource>,
    children: &[Node],
    locale: Option<&str>,
    i18n_catalog: Option<&Value>,
    data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
    _each_expr: &str,
) -> Result<Vec<Node>> {
    let Some(arr) = list else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in arr.iter() {
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
    data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<Vec<Node>> {
    let frag = registry.get(id).ok_or_else(|| {
        let msg = missing_fragment_message(id);
        match site {
            Some((file, source)) => {
                let dq = format!("id=\"{id}\"");
                let sq = format!("id='{id}'");
                Error::at(file, source, &[&dq, &sq], msg)
            }
            None => Error::at_file("<page>", msg),
        }
    })?;

    let frag_data =
        registry.resolve_fragment_data(frag, Some(prop_value), locale, data_cache, aliases)?;
    let local = ContextData::new(parent_data.clone()).with_links(&frag_data);

    // `data-bind="button"` → `button`; `data-bind="{a,b}"` → those fields.
    let bind_ctx = funnel::bind_context(&frag.bind, prop_value);
    let context_tree = ContextTree::new(ContextScope::Fragment, bind_ctx, local.clone());
    let ctx = context_tree.render_context();

    let mut nodes = fragment::template_children(frag);
    fill_attr_templates_in_nodes(&mut nodes, &ctx);
    fill_named_slots(&mut nodes, &ctx);
    fill_default_slots(&mut nodes, children);
    expand_usage_slots_in_nodes(
        registry,
        &mut nodes,
        Some(prop_value),
        local.as_map(),
        locale,
        i18n_catalog,
        data_cache,
        aliases,
        site,
    )?;
    let data_t_context = context_tree.translated_context(i18n_catalog);
    i18n::apply_data_t(&mut nodes, &data_t_context);
    Ok(nodes)
}
