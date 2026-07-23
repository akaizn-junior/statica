//! Font loading via `<link rel="statica/font">` → regular HTML5 `<link rel="stylesheet">`.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::aliases::{self, AliasOptions};
use crate::error::{Error, Result};
use crate::parse::{Document, Element, Node};

const GOOGLE_FONTS_ORIGIN: &str = "https://fonts.googleapis.com";
const GOOGLE_FONTS_STATIC: &str = "https://fonts.gstatic.com";

/// Expand every `<link rel="statica/font">` in the document.
///
/// Call after [`aliases::resolve_paths_in_document`] so `href` is already resolved.
pub fn expand_font_links(
    doc: &mut Document,
    _aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<()> {
    let mut state = ExpandState::default();
    expand_font_links_in_nodes(&mut doc.children, &mut state, site)
}

#[derive(Default)]
struct ExpandState {
    preconnect_done: HashSet<&'static str>,
}

fn expand_font_links_in_nodes(
    nodes: &mut Vec<Node>,
    state: &mut ExpandState,
    site: Option<(&str, &str)>,
) -> Result<()> {
    let mut i = 0;
    while i < nodes.len() {
        let is_font = matches!(&nodes[i], Node::Element(el) if is_font_link(el));
        if is_font {
            if let Node::Element(el) = &nodes[i] {
                let expanded = expand_font_link(el, state, site)?;
                let len = expanded.len();
                nodes.splice(i..=i, expanded);
                i += len.max(1);
                continue;
            }
        }
        if let Node::Element(el) = &mut nodes[i] {
            expand_font_links_in_nodes(&mut el.children, state, site)?;
        }
        i += 1;
    }
    Ok(())
}

fn expand_font_link(
    el: &Element,
    state: &mut ExpandState,
    site: Option<(&str, &str)>,
) -> Result<Vec<Node>> {
    let href = el.attr("href").unwrap_or("").trim();
    if href.is_empty() {
        return Err(font_err(site, &["href"], "statica/font link missing href"));
    }

    let mut out = Vec::new();

    if aliases::is_google_fonts_css(href) {
        push_preconnect(&mut out, GOOGLE_FONTS_ORIGIN, false, state);
        push_preconnect(&mut out, GOOGLE_FONTS_STATIC, true, state);
    }

    out.push(stylesheet_link(href, el));
    Ok(out)
}

fn push_preconnect(
    out: &mut Vec<Node>,
    href: &'static str,
    crossorigin: bool,
    state: &mut ExpandState,
) {
    if !state.preconnect_done.insert(href) {
        return;
    }
    if crossorigin {
        out.push(link_node(&[
            ("rel", "preconnect"),
            ("href", href),
            ("crossorigin", ""),
        ]));
    } else {
        out.push(link_node(&[("rel", "preconnect"), ("href", href)]));
    }
}

fn stylesheet_link(href: &str, src: &Element) -> Node {
    let mut attrs = IndexMap::new();
    attrs.insert("rel".into(), "stylesheet".into());
    attrs.insert("href".into(), href.to_string());
    for (k, v) in &src.attrs {
        if k == "rel" || k == "href" {
            continue;
        }
        attrs.insert(k.clone(), v.clone());
    }
    Node::Element(Element {
        name: "link".into(),
        attrs,
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

pub fn is_font_link(el: &Element) -> bool {
    el.is_link()
        && el
            .attr("rel")
            .is_some_and(|r| r.split_whitespace().any(|p| p == "statica/font"))
}

fn font_err(site: Option<(&str, &str)>, extra: &[&str], message: impl Into<String>) -> Error {
    let mut needles = vec!["rel=\"statica/font\"", "rel='statica/font'"];
    needles.extend_from_slice(extra);
    match site {
        Some((file, source)) => Error::at(file, source, &needles, message),
        None => Error::at_file("<page>", message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aliases::{resolve_paths_in_document, AliasOptions};
    use crate::parse::{parse_document, serialize_document};

    fn expand_doc(html: &str, aliases: &AliasOptions) -> String {
        let mut doc = parse_document(html).unwrap();
        resolve_paths_in_document(&mut doc, aliases, None).unwrap();
        expand_font_links(&mut doc, aliases, None).unwrap();
        serialize_document(&doc)
    }

    #[test]
    fn google_alias_expands_to_preconnect_and_stylesheet() {
        let html = expand_doc(
            r#"<!doctype html><html><head>
<link rel="statica/font" href="@Google/?family=Outfit:wght@100..900&display=swap" id="outfit-font" />
</head><body></body></html>"#,
            &AliasOptions::default(),
        );

        assert!(html.contains(r#"<link rel="preconnect" href="https://fonts.googleapis.com""#));
        assert!(html.contains(
            r#"<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="""#
        ));
        assert!(html.contains("fonts.googleapis.com/css2?family=Outfit:wght@100..900"));
        assert!(html.contains("display=swap"));
        assert!(html.contains(r#"rel="stylesheet""#));
        assert!(html.contains(r#"id="outfit-font""#));
        assert!(!html.contains("statica/font"));
    }

    #[test]
    fn preconnect_deduped_for_multiple_google_fonts() {
        let html = expand_doc(
            r#"<!doctype html><html><head>
<link rel="statica/font" href="@Google/?family=Outfit:wght@400&display=swap" />
<link rel="statica/font" href="@Google/?family=Open+Sans:wght@400;700&display=swap" />
</head><body></body></html>"#,
            &AliasOptions::default(),
        );

        assert_eq!(html.matches("rel=\"preconnect\"").count(), 2);
    }

    #[test]
    fn plain_local_path_rewrites_to_stylesheet() {
        let html = expand_doc(
            r#"<!doctype html><html><head>
<link rel="statica/font" href="./fonts/outfit.css" />
</head><body></body></html>"#,
            &AliasOptions::default(),
        );

        assert!(html.contains(r#"<link rel="stylesheet" href="./fonts/outfit.css""#));
        assert!(!html.contains("preconnect"));
        assert!(!html.contains("statica/font"));
    }

    #[test]
    fn local_alias_rewrites_to_stylesheet() {
        let mut aliases = AliasOptions::default();
        aliases
            .paths
            .insert("fonts".into(), "./assets/fonts".into());

        let html = expand_doc(
            r#"<!doctype html><html><head>
<link rel="statica/font" href="@fonts/outfit.css" />
</head><body></body></html>"#,
            &aliases,
        );

        assert!(html.contains(r#"href="./assets/fonts/outfit.css""#));
        assert!(html.contains(r#"rel="stylesheet""#));
    }
}
