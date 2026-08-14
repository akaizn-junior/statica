//! Internationalization: `[locale]` route expansion + `data-t` translation attrs.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::bind::expand_template;
use crate::discover::PageSource;
use crate::error::{Error, Result};
use crate::funnel::is_identifier;
use crate::parse::{Document, Node};
pub use crate::tokens::{DATA_T, DATA_T_ATTR_PREFIX};

/// Route param name for locale expansion (`[locale]/…`).
pub const LOCALE_PARAM: &str = "locale";

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
#[cfg(test)]
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
                "[i18n].locales is empty; add at least one locale, for example locales = [\"en\"]",
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

/// Whether the build should emit a root redirect to the default locale home.
///
/// True when i18n is enabled, the author did not define a root page, and the
/// default locale home was emitted (e.g. from `[locale]/index.html`).
#[must_use]
pub fn should_emit_root_redirect(i18n: &I18nOptions, pages: &[PageSource], out_dir: &Path) -> bool {
    if !i18n.enabled {
        return false;
    }
    if pages.iter().any(|p| p.route.is_home()) {
        return false;
    }
    out_dir
        .join(&i18n.default_locale)
        .join("index.html")
        .is_file()
}

/// Minimal root redirect stub for static hosts (meta refresh + JS fallback).
#[must_use]
pub fn root_redirect_html(default_locale: &str) -> String {
    let target = format!("/{default_locale}/");
    format!(
        r#"<!doctype html>
<html lang="{default_locale}">
  <head>
    <meta charset="utf-8" />
    <meta http-equiv="refresh" content="0; url={target}" />
    <link rel="canonical" href="{target}" />
    <title>Redirecting…</title>
    <script>location.replace("{target}" + location.hash);</script>
  </head>
  <body>
    <p><a href="{target}">Continue to site</a></p>
  </body>
</html>"#
    )
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

    pub fn root_keys(&self) -> Result<Vec<String>> {
        let mut keys = BTreeSet::new();
        for catalog in self.by_locale.values() {
            let Value::Object(map) = catalog else {
                continue;
            };
            for key in map.keys() {
                if !is_identifier(key) {
                    return Err(Error::at_file(
                        "i18n",
                        format!(
                            "i18n catalog key `{key}` cannot be used in data-t; top-level keys must be identifiers like `nav` or `about_title`"
                        ),
                    ));
                }
                keys.insert(key.clone());
            }
        }
        Ok(keys.into_iter().collect())
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
            format!(
                "missing i18n catalog at {}; create this file or remove the locale from [i18n].locales",
                path.display()
            ),
        ));
    }
    let text = fs::read_to_string(path).map_err(|e| Error::read(path.display().to_string(), e))?;
    serde_json::from_str(&text)
        .map_err(|e| Error::invalid_content(path.display().to_string(), e.to_string()))
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

/// Replace `data-t="text ${path}"` element content with the expanded template string.
///
/// `data-t-{attr}="text ${path}"` binds `{attr}` from the expanded template string.
pub fn apply_data_t(nodes: &mut [Node], context: &Value) {
    for node in nodes {
        if let Node::Element(el) = node {
            apply_data_t_on_element(el, context);
            apply_data_t(&mut el.children, context);
        }
    }
}

fn apply_data_t_on_element(el: &mut crate::parse::Element, context: &Value) {
    if let Some(text) = el.attrs.get(DATA_T).cloned() {
        el.children = vec![Node::Text(expand_template(&text, context))];
        el.attrs.shift_remove(DATA_T);
    }
    apply_data_t_attr_translations(el, context);
}

fn apply_data_t_attr_translations(el: &mut crate::parse::Element, context: &Value) {
    let markers: Vec<(String, String)> = el
        .attrs
        .iter()
        .filter_map(|(name, key)| {
            target_attr_from_data_t_key(name).map(|target| (target.to_string(), key.clone()))
        })
        .collect();

    for (target_attr, translation_key) in markers {
        let marker = format!("{DATA_T_ATTR_PREFIX}{target_attr}");
        el.attrs
            .insert(target_attr, expand_template(&translation_key, context));
        el.attrs.shift_remove(&marker);
    }
}

/// Remove `data-t` / `data-t-*` without translating — used when the parent page has no active locale.
#[cfg(test)]
pub fn strip_data_t(nodes: &mut [Node]) {
    for node in nodes {
        if let Node::Element(el) = node {
            strip_data_t_on_element(el);
            strip_data_t(&mut el.children);
        }
    }
}

#[cfg(test)]
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
            attrs: IndexMap::from([(DATA_T.into(), "${label}".into())]),
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
    fn data_t_attr_translates_href_and_content() {
        let catalog = json!({
            "canonical": { "href": "https://example.com/pt/sobre" },
            "description": { "content": "Página sobre nós" }
        });
        let mut nodes = vec![
            Node::Element(Element {
                name: "link".into(),
                attrs: IndexMap::from([
                    ("rel".into(), "canonical".into()),
                    ("href".into(), "https://example.com/about".into()),
                    ("data-t-href".into(), "${canonical.href}".into()),
                ]),
                children: vec![],
                void: true,
            }),
            Node::Element(Element {
                name: "meta".into(),
                attrs: IndexMap::from([
                    ("name".into(), "description".into()),
                    ("content".into(), "About us".into()),
                    ("data-t-content".into(), "${description.content}".into()),
                ]),
                children: vec![],
                void: true,
            }),
        ];
        apply_data_t(&mut nodes, &catalog);
        let link = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected link"),
        };
        let meta = match &nodes[1] {
            Node::Element(e) => e,
            _ => panic!("expected meta"),
        };
        assert_eq!(link.attr("href"), Some("https://example.com/pt/sobre"));
        assert_eq!(meta.attr("content"), Some("Página sobre nós"));
    }

    #[test]
    fn data_t_on_void_link_translates_inner_text_not_href() {
        let catalog = json!({"label": "Stylesheet"});
        let mut nodes = vec![Node::Element(Element {
            name: "link".into(),
            attrs: IndexMap::from([
                ("rel".into(), "stylesheet".into()),
                ("href".into(), "/site.css".into()),
                (DATA_T.into(), "${label}".into()),
            ]),
            children: vec![Node::Text("Site CSS".into())],
            void: true,
        })];
        apply_data_t(&mut nodes, &catalog);
        let link = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected link"),
        };
        assert_eq!(link.attr("href"), Some("/site.css"));
        assert!(matches!(&link.children[0], Node::Text(t) if t == "Stylesheet"));
    }

    #[test]
    fn data_t_translates_anchor_inner_text() {
        let catalog = json!({"nav": {"home": "Início"}});
        let mut nodes = vec![Node::Element(Element {
            name: "a".into(),
            attrs: IndexMap::from([
                ("href".into(), "/".into()),
                (DATA_T.into(), "${nav.home}".into()),
            ]),
            children: vec![Node::Text("Home".into())],
            void: false,
        })];
        apply_data_t(&mut nodes, &catalog);
        let link = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected anchor"),
        };
        assert_eq!(link.attr("href"), Some("/"));
        assert!(matches!(&link.children[0], Node::Text(t) if t == "Início"));
    }

    #[test]
    fn data_t_attr_translates_aria_label() {
        let catalog = json!({"nav": {"skip": "Saltar para o conteúdo"}});
        let mut nodes = vec![Node::Element(Element {
            name: "a".into(),
            attrs: IndexMap::from([
                ("href".into(), "#main".into()),
                ("aria-label".into(), "Skip to content".into()),
                ("data-t-aria-label".into(), "${nav.skip}".into()),
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
                    ("data-t-alt".into(), "${photo.alt}".into()),
                ]),
                children: vec![],
                void: true,
            }),
            Node::Element(Element {
                name: "input".into(),
                attrs: IndexMap::from([
                    ("type".into(), "email".into()),
                    ("placeholder".into(), "Your email".into()),
                    (
                        "data-t-placeholder".into(),
                        "${form.email_placeholder}".into(),
                    ),
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
                ("data-t-aria-label".into(), "${missing.key}".into()),
            ]),
            children: vec![Node::Text("×".into())],
            void: false,
        })];
        apply_data_t(&mut nodes, &catalog);
        let el = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("expected element"),
        };
        assert_eq!(el.attr("aria-label"), Some(""));
    }

    #[test]
    fn strip_data_t_removes_attr_markers() {
        let mut nodes = vec![Node::Element(Element {
            name: "img".into(),
            attrs: IndexMap::from([
                ("alt".into(), "Photo".into()),
                ("data-t-alt".into(), "${photo.alt}".into()),
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
    fn root_redirect_html_points_at_default_locale() {
        let html = root_redirect_html("pt");
        assert!(html.contains(r#"lang="pt""#));
        assert!(html.contains(r#"content="0; url=/pt/""#));
        assert!(html.contains(r#"location.replace("/pt/" + location.hash)"#));
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

    #[test]
    fn catalog_root_keys_must_be_identifiers() {
        let catalogs = I18nCatalogs {
            by_locale: HashMap::from([("en".into(), json!({"nav.home": "Home"}))]),
        };
        let err = catalogs.root_keys().unwrap_err();
        match err {
            Error::Diag(d) => assert!(d.message.contains("top-level keys must be identifiers")),
            other => panic!("unexpected: {other}"),
        }
    }
}
