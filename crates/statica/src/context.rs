//! Build-time context composition for pages and fragments.
//!
//! The build process owns canonical page data, but canonical roots are not
//! ambient globals. Pages opt in with `<html data-bind="...">`; fragments only
//! see their bound value and linked data sources.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::content::{DataKind, DataSet};
use crate::discover::PageSource;
use crate::funnel::{self, DataSource};
use crate::i18n;

/// Canonical page context roots produced by statica.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CanonicalRoot {
    /// All linked page data, keyed by data source id.
    Data,
    /// The current collection item for dynamic routes.
    Item,
    /// Route, params, and pagination metadata.
    Page,
    /// Active locale metadata and any bound translation catalog.
    I18n,
}

impl CanonicalRoot {
    /// Canonical roots in stable declaration order.
    pub const ALL: [Self; 4] = [Self::Data, Self::Item, Self::Page, Self::I18n];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Item => "item",
            Self::Page => "page",
            Self::I18n => "i18n",
        }
    }

    #[must_use]
    pub fn from_str(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|root| root.as_str() == value)
    }
}

/// Where a context is being used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextScope {
    /// Page templates may use bound page data and declared data link ids.
    Page,
    /// Fragments may use bound fragment data and linked data ids; no canonical fallback.
    Fragment,
}

/// A named layer in the context tree, ordered from highest to lowest precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextLayer {
    /// Values produced by `data-bind`.
    Bound,
    /// Values from valid `<link rel="statica/data" id="...">` sources.
    Linked,
}

impl ContextScope {
    #[must_use]
    pub const fn layers(self) -> &'static [ContextLayer] {
        match self {
            Self::Page | Self::Fragment => &[ContextLayer::Bound, ContextLayer::Linked],
        }
    }
}

/// Canonical page context built by statica before page binding.
#[derive(Debug, Clone)]
pub struct CanonicalContext {
    value: Value,
}

impl CanonicalContext {
    #[must_use]
    pub fn new(
        source: &PageSource,
        current: Option<&Value>,
        page_data: &HashMap<String, DataSource>,
        locale: Option<&str>,
    ) -> Self {
        let mut data = serde_json::Map::new();
        for (id, source) in page_data {
            data.insert(id.clone(), source.value());
        }

        let current = current.cloned().unwrap_or(Value::Null);
        let nested_item = current
            .get(CanonicalRoot::Item.as_str())
            .filter(|_| current.get("pagination").is_some())
            .cloned();
        let pagination = current.get("pagination").cloned();
        let render_item = nested_item.clone().unwrap_or_else(|| current.clone());
        let is_pagination = pagination.is_some()
            || current
                .as_object()
                .is_some_and(|obj| obj.contains_key("items") && obj.contains_key("total_pages"));

        let mut params = serde_json::Map::new();
        for param in &source.params {
            let value = if param == i18n::LOCALE_PARAM {
                locale.map_or(Value::Null, |loc| Value::String(loc.to_string()))
            } else {
                funnel::read_field(&current, param)
                    .or_else(|| {
                        nested_item
                            .as_ref()
                            .and_then(|item| funnel::read_field(item, param))
                    })
                    .cloned()
                    .unwrap_or(Value::Null)
            };
            params.insert(param.clone(), value);
        }

        let mut page = serde_json::Map::new();
        page.insert("route".into(), Value::String(source.route.clone()));
        page.insert("params".into(), Value::Object(params));
        if is_pagination {
            page.insert(
                "pagination".into(),
                pagination.unwrap_or_else(|| current.clone()),
            );
        }

        let mut value = serde_json::Map::new();
        value.insert(CanonicalRoot::Data.as_str().into(), Value::Object(data));
        value.insert(
            CanonicalRoot::Item.as_str().into(),
            if is_pagination && nested_item.is_none() {
                Value::Null
            } else {
                render_item
            },
        );
        value.insert(CanonicalRoot::Page.as_str().into(), Value::Object(page));
        value.insert(
            CanonicalRoot::I18n.as_str().into(),
            serde_json::json!({ "locale": locale.unwrap_or("") }),
        );

        Self {
            value: Value::Object(value),
        }
    }

    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    #[must_use]
    pub fn as_data_sources(&self, page_data: &HashMap<String, DataSource>) -> ContextData {
        let mut out = page_data.clone();
        for root in CanonicalRoot::ALL {
            let id = root.as_str();
            let value = funnel::read_field(&self.value, id)
                .cloned()
                .unwrap_or(Value::Null);
            out.insert(
                id.to_string(),
                DataSource {
                    id: id.to_string(),
                    kind: DataKind::Json,
                    path: PathBuf::from(format!("statica:{id}")),
                    data: DataSet::Json(value),
                },
            );
        }
        ContextData(out)
    }
}

/// Data sources available to mount expressions and nested fragment expansion.
#[derive(Debug, Clone)]
pub struct ContextData(HashMap<String, DataSource>);

impl ContextData {
    #[must_use]
    pub const fn new(data: HashMap<String, DataSource>) -> Self {
        Self(data)
    }

    #[must_use]
    pub const fn as_map(&self) -> &HashMap<String, DataSource> {
        &self.0
    }

    #[must_use]
    pub fn with_links(&self, links: &HashMap<String, DataSource>) -> Self {
        let mut out = self.0.clone();
        for (id, source) in links {
            out.insert(id.clone(), source.clone());
        }
        Self(out)
    }
}

/// Ordered tree of context layers for one render scope.
#[derive(Debug, Clone)]
pub struct ContextTree {
    scope: ContextScope,
    bound: Value,
    data: ContextData,
}

impl ContextTree {
    #[must_use]
    pub const fn new(scope: ContextScope, bound: Value, data: ContextData) -> Self {
        Self { scope, bound, data }
    }

    #[must_use]
    pub fn render_context(&self) -> Value {
        let mut roots = serde_json::Map::new();
        for layer in self.scope.layers() {
            for (key, value) in self.roots(*layer) {
                roots.entry(key).or_insert(value);
            }
        }
        Value::Object(roots)
    }

    #[must_use]
    pub fn translated_context(&self, catalog: Option<&Value>) -> Value {
        let mut ctx = match self.render_context() {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        let i18n_root = CanonicalRoot::I18n.as_str();
        if let (Some(base), Some(catalog)) = (ctx.get(i18n_root), catalog) {
            ctx.insert(i18n_root.into(), deep_merge(base, catalog));
        }
        Value::Object(ctx)
    }

    fn roots(&self, layer: ContextLayer) -> serde_json::Map<String, Value> {
        match layer {
            ContextLayer::Bound => value_roots(&self.bound),
            ContextLayer::Linked => linked_roots(self.data.as_map()),
        }
    }
}

fn value_roots(value: &Value) -> serde_json::Map<String, Value> {
    match value {
        Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    }
}

fn linked_roots(data: &HashMap<String, DataSource>) -> serde_json::Map<String, Value> {
    let mut roots = serde_json::Map::new();
    for (id, source) in data {
        if CanonicalRoot::from_str(id).is_none() {
            roots.insert(id.clone(), source.value());
        }
    }
    roots
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source(id: &str, value: Value) -> DataSource {
        DataSource {
            id: id.to_string(),
            kind: DataKind::Json,
            path: PathBuf::from(format!("{id}.json")),
            data: DataSet::Json(value),
        }
    }

    #[test]
    fn page_bound_roots_precede_linked_roots() {
        let data = ContextData::new(HashMap::from([(
            "title".into(),
            source("title", json!("linked")),
        )]));
        let tree = ContextTree::new(ContextScope::Page, json!({"title": "bound"}), data);

        assert_eq!(tree.render_context(), json!({"title": "bound"}));
    }

    #[test]
    fn fragment_bound_roots_precede_linked_roots_without_canonical_fallback() {
        let data = ContextData::new(HashMap::from([
            ("label".into(), source("label", json!("linked"))),
            (
                "item".into(),
                source("item", json!({"headline": "canonical"})),
            ),
        ]));
        let tree = ContextTree::new(ContextScope::Fragment, json!({"label": "bound"}), data);

        assert_eq!(tree.render_context(), json!({"label": "bound"}));
    }

    #[test]
    fn i18n_catalog_merges_only_when_i18n_is_bound() {
        let data = ContextData::new(HashMap::new());
        let unbound = ContextTree::new(ContextScope::Page, json!({}), data.clone());
        assert_eq!(
            unbound.translated_context(Some(&json!({"title": "Home"}))),
            json!({})
        );

        let bound = ContextTree::new(ContextScope::Page, json!({"i18n": {"locale": "en"}}), data);
        assert_eq!(
            bound.translated_context(Some(&json!({"title": "Home"}))),
            json!({"i18n": {"locale": "en", "title": "Home"}})
        );
    }
}
