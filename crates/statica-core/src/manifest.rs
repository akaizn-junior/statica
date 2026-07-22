//! Web app manifest — scaffold `public/manifest.webmanifest` and inject PWA head tags.
//!
//! Enable with `manifest = true` in statica.toml or `--manifest` on the CLI. statica
//! creates a starter manifest file when missing (edit it directly), copies it via
//! `asset_dirs`, and adds `<link rel="manifest">` plus related tags to every page.

use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::parse::{Document, Element, Node};

/// Project-relative path to the editable manifest file.
pub const MANIFEST_FILE: &str = "public/manifest.webmanifest";

/// URL path emitted in HTML (manifest lives at site root after copy from `public/`).
pub const MANIFEST_HREF: &str = "/manifest.webmanifest";

const DEFAULT_MANIFEST: &str = r##"{
  "name": "My Site",
  "short_name": "Site",
  "description": "",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#ffffff",
  "theme_color": "#111827",
  "icons": [
    {
      "src": "/icon-192.png",
      "sizes": "192x192",
      "type": "image/png",
      "purpose": "any"
    }
  ]
}
"##;

/// Values read from the manifest JSON for automatic head-tag injection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestMeta {
    pub theme_color: Option<String>,
    pub apple_touch_icon: Option<String>,
}

/// Create `public/manifest.webmanifest` when `--manifest` is on and the file is missing.
pub fn ensure_manifest_file(site_root: &Path) -> Result<PathBuf> {
    let path = site_root.join(MANIFEST_FILE);
    if path.exists() {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, DEFAULT_MANIFEST)?;
    Ok(path)
}

/// Parse theme color and a default icon from the manifest file.
pub fn read_manifest_meta(path: &Path) -> Result<ManifestMeta> {
    let text = fs::read_to_string(path)
        .map_err(|e| Error::at_file(path.display().to_string(), e.to_string()))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| {
        Error::at_file(
            path.display().to_string(),
            format!("invalid manifest JSON: {e}"),
        )
    })?;
    Ok(ManifestMeta {
        theme_color: value
            .get("theme_color")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        apple_touch_icon: value
            .get("icons")
            .and_then(Value::as_array)
            .and_then(|icons| {
                icons.iter().find_map(|icon| {
                    let purpose = icon.get("purpose").and_then(Value::as_str).unwrap_or("");
                    if !purpose.is_empty()
                        && !purpose.split_whitespace().any(|p| p == "any")
                    {
                        return None;
                    }
                    icon.get("src")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                })
            }),
    })
}

/// Inject manifest / theme-color / apple-touch-icon tags into `<head>` when absent.
pub fn inject_manifest_tags(doc: &mut Document, meta: &ManifestMeta) {
    let Some(head) = find_head(doc) else {
        return;
    };
    if !head_has_link_rel(head, "manifest") {
        head.children
            .push(link_node(&[("rel", "manifest"), ("href", MANIFEST_HREF)]));
    }
    if let Some(color) = meta.theme_color.as_deref() {
        if !head_has_meta_name(head, "theme-color") {
            head.children.push(meta_node("theme-color", color));
        }
    }
    if let Some(icon) = meta.apple_touch_icon.as_deref() {
        if !head_has_link_rel(head, "apple-touch-icon") {
            head.children
                .push(link_node(&[("rel", "apple-touch-icon"), ("href", icon)]));
        }
    }
}

fn find_head(doc: &mut Document) -> Option<&mut Element> {
    for node in &mut doc.children {
        if let Node::Element(html) = node {
            if !html.name.eq_ignore_ascii_case("html") {
                continue;
            }
            for child in &mut html.children {
                if let Node::Element(head) = child {
                    if head.name.eq_ignore_ascii_case("head") {
                        return Some(head);
                    }
                }
            }
        }
    }
    None
}

fn head_has_link_rel(head: &Element, rel: &str) -> bool {
    head.children.iter().any(|node| {
        matches!(node, Node::Element(el) if el.is_link()
            && el.attr("rel").is_some_and(|r| r.split_whitespace().any(|p| p == rel)))
    })
}

fn head_has_meta_name(head: &Element, name: &str) -> bool {
    head.children.iter().any(|node| {
        matches!(node, Node::Element(el) if el.name.eq_ignore_ascii_case("meta")
            && el.attr("name").is_some_and(|n| n.eq_ignore_ascii_case(name)))
    })
}

fn meta_node(name: &str, content: &str) -> Node {
    Node::Element(Element {
        name: "meta".into(),
        attrs: IndexMap::from([
            ("name".into(), name.into()),
            ("content".into(), content.into()),
        ]),
        children: Vec::new(),
        void: true,
    })
}

fn link_node(attrs: &[(&str, &str)]) -> Node {
    let mut map = IndexMap::new();
    for (k, v) in attrs {
        map.insert((*k).into(), (*v).into());
    }
    Node::Element(Element {
        name: "link".into(),
        attrs: map,
        children: Vec::new(),
        void: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse_document, serialize_document};

    #[test]
    fn scaffolds_manifest_file() {
        let dir = std::env::temp_dir().join(format!(
            "statica-manifest-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = ensure_manifest_file(&dir).unwrap();
        assert!(path.exists());
        let value: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["name"], "My Site");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn injects_tags_from_meta() {
        let mut doc = parse_document(
            r#"<!doctype html><html><head><meta charset="utf-8" /></head><body></body></html>"#,
        )
        .unwrap();
        inject_manifest_tags(
            &mut doc,
            &ManifestMeta {
                theme_color: Some("#111827".into()),
                apple_touch_icon: Some("/icon-192.png".into()),
            },
        );
        let html = serialize_document(&doc);
        assert!(html.contains(r#"<link rel="manifest" href="/manifest.webmanifest""#));
        assert!(html.contains(r##"<meta name="theme-color" content="#111827""##));
        assert!(html.contains(r#"<link rel="apple-touch-icon" href="/icon-192.png""#));
    }

    #[test]
    fn skips_existing_tags() {
        let mut doc = parse_document(
            r##"<!doctype html><html><head>
<link rel="manifest" href="/custom.webmanifest" />
<meta name="theme-color" content="#000" />
</head><body></body></html>"##,
        )
        .unwrap();
        inject_manifest_tags(
            &mut doc,
            &ManifestMeta {
                theme_color: Some("#111827".into()),
                apple_touch_icon: Some("/icon-192.png".into()),
            },
        );
        let html = serialize_document(&doc);
        assert!(html.contains("/custom.webmanifest"));
        assert!(!html.contains("/manifest.webmanifest"));
        assert!(html.contains(r##"content="#000""##));
        assert!(html.contains(r#"<link rel="apple-touch-icon" href="/icon-192.png""#));
    }
}
