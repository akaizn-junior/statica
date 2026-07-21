//! Internationalization: `[locale]` route expansion + `data-t` translation attrs.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::funnel::{parse_js_value, read_field};
use crate::parse::{Document, Node};

/// Route param name for locale expansion (`[locale]/…`).
pub const LOCALE_PARAM: &str = "locale";

/// i18n settings mapped from `[i18n]` in `statica.toml`.
#[derive(Debug, Clone)]
pub struct I18nOptions {
    pub enabled: bool,
    /// Default locale (fallback catalog; used for pages without `[locale]` in the route).
    pub default_locale: String,
    /// Locales emitted for every `[locale]/…` page template.
    pub locales: Vec<String>,
    /// Directory under the site root with `{locale}.json` catalogs.
    pub dir: String,
    /// Fallback catalog when a key is missing. Empty → `default_locale`.
    pub fallback: String,
}

impl Default for I18nOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            default_locale: "en".into(),
            locales: vec!["en".into()],
            dir: "content/i18n".into(),
            fallback: String::new(),
        }
    }
}

impl I18nOptions {
    #[must_use]
    pub fn effective_fallback(&self) -> &str {
        if self.fallback.is_empty() {
            self.default_locale.as_str()
        } else {
            self.fallback.as_str()
        }
    }

    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.locales.is_empty() {
            return Err(Error::at_file(
                "statica.toml",
                "[i18n].locales must list at least one locale when i18n is enabled",
            ));
        }
        if !self.locales.iter().any(|l| l == &self.default_locale) {
            return Err(Error::at_file(
                "statica.toml",
                format!(
                    "[i18n].default `{}` must appear in locales ({})",
                    self.default_locale,
                    self.locales.join(", ")
                ),
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn route_has_locale<'a>(&self, params: impl IntoIterator<Item = &'a str>) -> bool {
        self.enabled && params.into_iter().any(|p| p == LOCALE_PARAM)
    }
}

/// Loaded translation catalogs keyed by locale code.
#[derive(Debug, Clone, Default)]
pub struct I18nCatalogs {
    pub by_locale: HashMap<String, Value>,
}

impl I18nCatalogs {
    /// Load catalogs for every configured locale (+ fallback when distinct).
    pub fn load(root: &Path, opts: &I18nOptions) -> Result<Self> {
        opts.validate()?;
        if !opts.enabled {
            return Ok(Self::default());
        }

        let dir = catalog_dir(root, opts);
        let mut needed: Vec<String> = opts.locales.clone();
        let fb = opts.effective_fallback().to_string();
        if !needed.iter().any(|l| l == &fb) {
            needed.push(fb);
        }

        let mut by_locale = HashMap::new();
        for locale in needed {
            let path = catalog_path(&dir, &locale);
            let value = read_catalog(&path)?;
            by_locale.insert(locale, value);
        }
        Ok(Self { by_locale })
    }

    #[must_use]
    pub fn for_locale(&self, locale: &str, opts: &I18nOptions) -> Value {
        resolve_catalog(&self.by_locale, locale, opts.effective_fallback())
    }
}

/// Bind context with `locale` for `${locale}` in attributes.
#[must_use]
pub fn locale_bind_context(locale: &str) -> Value {
    let mut map = Map::new();
    map.insert(LOCALE_PARAM.into(), Value::String(locale.to_string()));
    Value::Object(map)
}

/// Merge `locale` into a collection item for attribute templates.
#[must_use]
pub fn merge_locale_into(item: &Value, locale: &str) -> Value {
    match item {
        Value::Object(map) => {
            let mut out = map.clone();
            out.insert(LOCALE_PARAM.into(), Value::String(locale.to_string()));
            Value::Object(out)
        }
        _ => locale_bind_context(locale),
    }
}

fn catalog_dir(root: &Path, opts: &I18nOptions) -> PathBuf {
    if Path::new(&opts.dir).is_absolute() {
        PathBuf::from(&opts.dir)
    } else {
        root.join(&opts.dir)
    }
}

fn catalog_path(dir: &Path, locale: &str) -> PathBuf {
    dir.join(format!("{locale}.json"))
}

fn read_catalog(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Err(Error::at_file(
            path.display().to_string(),
            format!("missing i18n catalog: {}", path.display()),
        ));
    }
    let text = fs::read_to_string(path)
        .map_err(|e| Error::read(path.display().to_string(), e))?;
    parse_js_value(&text)
        .map_err(|message| Error::invalid_js_value(path.display().to_string(), message))
}

fn resolve_catalog(catalogs: &HashMap<String, Value>, locale: &str, fallback: &str) -> Value {
    let primary = catalogs
        .get(locale)
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    if locale == fallback {
        return primary;
    }
    let fb = catalogs
        .get(fallback)
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    deep_merge(&fb, &primary)
}

fn deep_merge(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            let mut out = base_map.clone();
            for (key, value) in overlay_map {
                out.insert(
                    key.clone(),
                    match (out.get(key), value) {
                        (Some(existing), overlay_val)
                            if existing.is_object() && overlay_val.is_object() =>
                        {
                            deep_merge(existing, overlay_val)
                        }
                        (_, overlay_val) => overlay_val.clone(),
                    },
                );
            }
            Value::Object(out)
        }
        (_, overlay) => overlay.clone(),
    }
}

/// Look up a dotted translation key in a catalog object.
#[must_use]
pub fn lookup_key(catalog: &Value, key: &str) -> Option<String> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let mut cur = catalog;
    let mut parts = key.split('.').filter(|p| !p.is_empty());
    let first = parts.next()?;
    cur = read_field(cur, first)?;
    for part in parts {
        cur = read_field(cur, part)?;
    }
    match cur {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Replace `data-t="key"` element content with the catalog string (fallback: inner text).
pub fn apply_data_t(nodes: &mut [Node], catalog: &Value) {
    for node in nodes {
        if let Node::Element(el) = node {
            if let Some(key) = el.attrs.get("data-t").cloned() {
                let fallback = direct_text_content(&el.children);
                let text = lookup_key(catalog, &key).unwrap_or(fallback);
                el.children = vec![Node::Text(text)];
                el.attrs.shift_remove("data-t");
            }
            apply_data_t(&mut el.children, catalog);
        }
    }
}

fn direct_text_content(nodes: &[Node]) -> String {
    nodes
        .iter()
        .filter_map(|n| match n {
            Node::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// Set `<html lang="…">` for the active locale.
pub fn set_html_lang(doc: &mut Document, locale: &str) {
    for child in &mut doc.children {
        if let Node::Element(el) = child {
            if el.name.eq_ignore_ascii_case("html") {
                el.attrs.insert("lang".into(), locale.to_string());
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Element, Node};
    use indexmap::IndexMap;
    use serde_json::json;

    #[test]
    fn route_has_locale_param() {
        let opts = I18nOptions {
            enabled: true,
            ..Default::default()
        };
        assert!(opts.route_has_locale(["locale"]));
        assert!(opts.route_has_locale(["locale", "slug"]));
        assert!(!opts.route_has_locale(["slug"]));
    }

    #[test]
    fn data_t_replaces_text_and_strips_attr() {
        let catalog = json!({"label": "Olá"});
        let mut nodes = vec![Node::Element(Element {
            name: "span".into(),
            attrs: IndexMap::from([("data-t".into(), "label".into())]),
            children: vec![Node::Text("hello".into())],
            void: false,
        })];
        apply_data_t(&mut nodes, &catalog);
        let el = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected element"),
        };
        assert!(!el.attrs.contains_key("data-t"));
        assert!(matches!(&el.children[0], Node::Text(t) if t == "Olá"));
    }

    #[test]
    fn loads_all_configured_locales() {
        let dir = std::env::temp_dir().join(format!("statica-i18n-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("content/i18n")).unwrap();
        fs::write(dir.join("content/i18n/en.json"), r#"{"title": "Home"}"#).unwrap();
        fs::write(dir.join("content/i18n/pt.json"), r#"{"title": "Início"}"#).unwrap();

        let opts = I18nOptions {
            enabled: true,
            locales: vec!["en".into(), "pt".into()],
            ..Default::default()
        };
        let catalogs = I18nCatalogs::load(&dir, &opts).unwrap();
        assert_eq!(catalogs.by_locale.len(), 2);
        assert_eq!(catalogs.for_locale("pt", &opts)["title"], "Início");

        let _ = fs::remove_dir_all(dir);
    }
}
