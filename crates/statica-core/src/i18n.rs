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

/// Token in funnel `src` paths resolved to the active locale at build time.
pub const LOCALE_SRC_TOKEN: &str = "${locale}";

/// Translate element text content from the catalog.
pub const DATA_T: &str = "data-t";

/// Prefix for per-attribute translation markers: `data-t-{attr}` → `{attr}`.
pub const DATA_T_ATTR_PREFIX: &str = "data-t-";

/// Accessibility-related attributes that authors typically translate via `data-t-{attr}`.
pub const A11Y_TRANSLATABLE_ATTRS: &[&str] = &[
    "alt",
    "aria-braillelabel",
    "aria-brailleroledescription",
    "aria-description",
    "aria-errormessage",
    "aria-label",
    "aria-placeholder",
    "aria-roledescription",
    "aria-valuetext",
    "placeholder",
    "title",
];

/// Whether an attribute name is a `data-t-{target}` translation marker.
#[must_use]
pub fn is_data_t_attr(name: &str) -> bool {
    name.starts_with(DATA_T_ATTR_PREFIX) && name.len() > DATA_T_ATTR_PREFIX.len()
}

/// Target attribute for a `data-t-{target}` marker (e.g. `data-t-aria-label` → `aria-label`).
#[must_use]
pub fn target_attr_from_data_t_key(name: &str) -> Option<&str> {
    name.strip_prefix(DATA_T_ATTR_PREFIX)
        .filter(|target| !target.is_empty())
}

/// Whether a funnel `src` path contains a locale token.
#[must_use]
pub fn src_has_locale_token(src: &str) -> bool {
    src.contains(LOCALE_SRC_TOKEN)
}

/// Replace `${locale}` in a funnel `src` path with the active locale code.
#[must_use]
pub fn interpolate_locale(src: &str, locale: &str) -> String {
    src.replace(LOCALE_SRC_TOKEN, locale)
}

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
/// For `<meta>` / `<link>`, updates `content` or `href` instead.
///
/// `data-t-{attr}="key"` translates `{attr}` from the catalog (fallback: current `{attr}` value).
pub fn apply_data_t(nodes: &mut [Node], catalog: &Value) {
    for node in nodes {
        if let Node::Element(el) = node {
            apply_data_t_on_element(el, catalog);
            apply_data_t(&mut el.children, catalog);
        }
    }
}

fn apply_data_t_on_element(el: &mut crate::parse::Element, catalog: &Value) {
    if let Some(key) = el.attrs.get(DATA_T).cloned() {
        if el.name.eq_ignore_ascii_case("link") {
            let fallback = el.attrs.get("href").cloned().unwrap_or_default();
            let text = lookup_key(catalog, &key).unwrap_or(fallback);
            el.attrs.insert("href".into(), text);
        } else if el.name.eq_ignore_ascii_case("meta") {
            let fallback = el.attrs.get("content").cloned().unwrap_or_default();
            let text = lookup_key(catalog, &key).unwrap_or(fallback);
            el.attrs.insert("content".into(), text);
        } else {
            let fallback = direct_text_content(&el.children);
            let text = lookup_key(catalog, &key).unwrap_or(fallback);
            el.children = vec![Node::Text(text)];
        }
        el.attrs.shift_remove(DATA_T);
    }
    apply_data_t_attr_translations(el, catalog);
}

fn apply_data_t_attr_translations(el: &mut crate::parse::Element, catalog: &Value) {
    let markers: Vec<(String, String)> = el
        .attrs
        .iter()
        .filter_map(|(name, key)| {
            target_attr_from_data_t_key(name).map(|target| (target.to_string(), key.clone()))
        })
        .collect();

    for (target_attr, translation_key) in markers {
        let marker = format!("{DATA_T_ATTR_PREFIX}{target_attr}");
        let fallback = el
            .attrs
            .get(&target_attr)
            .cloned()
            .unwrap_or_default();
        let text = lookup_key(catalog, &translation_key).unwrap_or(fallback);
        el.attrs.insert(target_attr, text);
        el.attrs.shift_remove(&marker);
    }
}

/// Remove `data-t` / `data-t-*` without translating — used when the parent page has no active locale.
pub fn strip_data_t(nodes: &mut [Node]) {
    for node in nodes {
        if let Node::Element(el) = node {
            strip_data_t_on_element(el);
            strip_data_t(&mut el.children);
        }
    }
}

fn strip_data_t_on_element(el: &mut crate::parse::Element) {
    el.attrs.shift_remove(DATA_T);
    let markers: Vec<String> = el
        .attrs
        .keys()
        .filter(|name| is_data_t_attr(name))
        .cloned()
        .collect();
    for marker in markers {
        el.attrs.shift_remove(&marker);
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
    fn interpolate_locale_in_src() {
        assert!(!src_has_locale_token("posts.json"));
        assert!(src_has_locale_token("../content/posts.${locale}.json"));
        assert_eq!(
            interpolate_locale("../content/posts.${locale}.json", "pt"),
            "../content/posts.pt.json"
        );
    }

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
    fn strip_data_t_removes_attr_keeps_content() {
        let mut nodes = vec![Node::Element(Element {
            name: "button".into(),
            attrs: IndexMap::from([("data-t".into(), "label".into())]),
            children: vec![Node::Text("Send".into())],
            void: false,
        })];
        strip_data_t(&mut nodes);
        let el = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected element"),
        };
        assert!(!el.attrs.contains_key("data-t"));
        assert!(matches!(&el.children[0], Node::Text(t) if t == "Send"));
    }

    #[test]
    fn data_t_replaces_text_and_strips_attr() {
        let catalog = json!({"label": "Olá"});
        let mut nodes = vec![Node::Element(Element {
            name: "span".into(),
            attrs: IndexMap::from([(DATA_T.into(), "label".into())]),
            children: vec![Node::Text("hello".into())],
            void: false,
        })];
        apply_data_t(&mut nodes, &catalog);
        let el = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected element"),
        };
        assert!(!el.attrs.contains_key(DATA_T));
        assert!(matches!(&el.children[0], Node::Text(t) if t == "Olá"));
    }

    #[test]
    fn data_t_attr_translates_aria_label() {
        let catalog = json!({"nav": {"skip": "Saltar para o conteúdo"}});
        let mut nodes = vec![Node::Element(Element {
            name: "a".into(),
            attrs: IndexMap::from([
                ("href".into(), "#main".into()),
                ("aria-label".into(), "Skip to content".into()),
                ("data-t-aria-label".into(), "nav.skip".into()),
            ]),
            children: vec![],
            void: false,
        })];
        apply_data_t(&mut nodes, &catalog);
        let el = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected element"),
        };
        assert_eq!(el.attr("aria-label"), Some("Saltar para o conteúdo"));
        assert!(!el.attrs.contains_key("data-t-aria-label"));
    }

    #[test]
    fn data_t_attr_translates_alt_and_placeholder() {
        let catalog = json!({
            "photo": { "alt": "Pôr do sol" },
            "form": { "email_placeholder": "O seu email" }
        });
        let mut nodes = vec![
            Node::Element(Element {
                name: "img".into(),
                attrs: IndexMap::from([
                    ("src".into(), "sunset.jpg".into()),
                    ("alt".into(), "Sunset".into()),
                    ("data-t-alt".into(), "photo.alt".into()),
                ]),
                children: vec![],
                void: true,
            }),
            Node::Element(Element {
                name: "input".into(),
                attrs: IndexMap::from([
                    ("type".into(), "email".into()),
                    ("placeholder".into(), "Your email".into()),
                    ("data-t-placeholder".into(), "form.email_placeholder".into()),
                ]),
                children: vec![],
                void: true,
            }),
        ];
        apply_data_t(&mut nodes, &catalog);
        let img = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected img"),
        };
        let input = match &nodes[1] {
            Node::Element(e) => e,
            _ => panic!("expected input"),
        };
        assert_eq!(img.attr("alt"), Some("Pôr do sol"));
        assert_eq!(input.attr("placeholder"), Some("O seu email"));
    }

    #[test]
    fn data_t_attr_falls_back_to_existing_attr_value() {
        let catalog = json!({});
        let mut nodes = vec![Node::Element(Element {
            name: "button".into(),
            attrs: IndexMap::from([
                ("aria-label".into(), "Close dialog".into()),
                ("data-t-aria-label".into(), "missing.key".into()),
            ]),
            children: vec![Node::Text("×".into())],
            void: false,
        })];
        apply_data_t(&mut nodes, &catalog);
        let el = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected element"),
        };
        assert_eq!(el.attr("aria-label"), Some("Close dialog"));
    }

    #[test]
    fn strip_data_t_removes_attr_markers() {
        let mut nodes = vec![Node::Element(Element {
            name: "img".into(),
            attrs: IndexMap::from([
                ("alt".into(), "Photo".into()),
                ("data-t-alt".into(), "photo.alt".into()),
            ]),
            children: vec![],
            void: true,
        })];
        strip_data_t(&mut nodes);
        let el = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected element"),
        };
        assert!(!el.attrs.contains_key("data-t-alt"));
        assert_eq!(el.attr("alt"), Some("Photo"));
    }

    #[test]
    fn a11y_translatable_attrs_are_data_t_targets() {
        for attr in A11Y_TRANSLATABLE_ATTRS {
            let marker = format!("{DATA_T_ATTR_PREFIX}{attr}");
            assert!(is_data_t_attr(&marker));
            assert_eq!(target_attr_from_data_t_key(&marker), Some(*attr));
        }
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
