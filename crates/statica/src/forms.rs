//! Static forms — wire `<form statica>` to a configured provider endpoint at build time.
//!
//! No client JS is injected; emitted forms use native `action` + `method="POST"`.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::parse::{Document, Element, Node};

/// Form provider backend (from `[forms]` in statica.toml).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormProvider {
    #[default]
    Formspree,
    Custom,
}

/// Build-time form wiring (mapped from `[forms]` in statica.toml).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormsOptions {
    pub enabled: bool,
    pub provider: FormProvider,
    /// Formspree: template with `{id}`. Custom: single POST URL for every form.
    pub endpoint: String,
    /// Logical form name → provider form id (Formspree) or unused (Custom).
    pub ids: HashMap<String, String>,
}

impl Default for FormsOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: FormProvider::Formspree,
            endpoint: "https://formspree.io/f/{id}".into(),
            ids: HashMap::new(),
        }
    }
}

impl FormsOptions {
    #[must_use]
    pub fn provider_from_str(s: &str) -> FormProvider {
        match s.trim().to_ascii_lowercase().as_str() {
            "custom" => FormProvider::Custom,
            _ => FormProvider::Formspree,
        }
    }
}

/// Wire every `<form statica>` in the document when [`FormsOptions::enabled`].
pub fn wire_forms_in_document(
    doc: &mut Document,
    forms: &FormsOptions,
    site: Option<(&str, &str)>,
) -> Result<()> {
    if !forms.enabled {
        return Ok(());
    }
    wire_forms_in_nodes(&mut doc.children, forms, site)
}

fn wire_forms_in_nodes(
    nodes: &mut [Node],
    forms: &FormsOptions,
    site: Option<(&str, &str)>,
) -> Result<()> {
    for node in nodes {
        if let Node::Element(el) = node {
            if is_statica_form(el) {
                wire_form(el, forms, site)?;
            }
            wire_forms_in_nodes(&mut el.children, forms, site)?;
        }
    }
    Ok(())
}

fn is_statica_form(el: &Element) -> bool {
    el.name.eq_ignore_ascii_case("form") && el.attrs.contains_key("statica")
}

fn form_key(el: &Element) -> Option<&str> {
    el.attr("name").or_else(|| el.attr("id"))
}

fn wire_form(el: &mut Element, forms: &FormsOptions, site: Option<(&str, &str)>) -> Result<()> {
    let key = form_key(el).ok_or_else(|| {
        form_err(
            site,
            &["statica", "name=", "id="],
            "statica form requires a name or id attribute for [forms.ids] lookup",
        )
    })?;

    let action = match forms.provider {
        FormProvider::Formspree => {
            let id = forms.ids.get(key).ok_or_else(|| {
                form_err(
                    site,
                    &[&format!("name=\"{key}\""), &format!("id=\"{key}\""), "statica"],
                    format!("no [forms.ids] entry for form `{key}`"),
                )
            })?;
            if !forms.endpoint.contains("{id}") {
                return Err(form_err(
                    site,
                    &["statica"],
                    "forms endpoint must contain `{id}` for provider \"formspree\"",
                ));
            }
            forms.endpoint.replace("{id}", id)
        }
        FormProvider::Custom => {
            if forms.endpoint.is_empty() {
                return Err(form_err(
                    site,
                    &["statica"],
                    "forms endpoint is empty for provider \"custom\"",
                ));
            }
            forms.endpoint.clone()
        }
    };

    el.attrs.shift_remove("statica");
    el.attrs.insert("action".into(), action);
    el.attrs.insert("method".into(), "POST".into());
    Ok(())
}

fn form_err(
    site: Option<(&str, &str)>,
    needles: &[&str],
    message: impl Into<String>,
) -> Error {
    match site {
        Some((file, source)) => Error::at(file, source, needles, message),
        None => Error::at_file("<page>", message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formspree(ids: &[(&str, &str)]) -> FormsOptions {
        FormsOptions {
            enabled: true,
            provider: FormProvider::Formspree,
            endpoint: "https://formspree.io/f/{id}".into(),
            ids: ids
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn wires_formspree_form_by_name() {
        let mut doc = crate::parse::parse_document(
            r#"<!doctype html><html><body>
<form name="contact" statica><input name="email" /></form>
</body></html>"#,
        )
        .unwrap();
        wire_forms_in_document(
            &mut doc,
            &formspree(&[("contact", "xyzabc")]),
            None,
        )
        .unwrap();
        let html = crate::parse::serialize_document(&doc);
        assert!(html.contains(r#"action="https://formspree.io/f/xyzabc""#));
        assert!(html.contains(r#"method="POST""#));
        assert!(!html.contains("statica"));
    }

    #[test]
    fn wires_form_by_id_when_name_missing() {
        let mut doc = crate::parse::parse_document(
            r#"<form id="newsletter" statica><button>Go</button></form>"#,
        )
        .unwrap();
        wire_forms_in_document(
            &mut doc,
            &formspree(&[("newsletter", "abc123")]),
            None,
        )
        .unwrap();
        let html = crate::parse::serialize_document(&doc);
        assert!(html.contains("https://formspree.io/f/abc123"));
    }

    #[test]
    fn custom_provider_sets_single_endpoint() {
        let mut doc = crate::parse::parse_document(
            r#"<form name="contact" statica></form>"#,
        )
        .unwrap();
        let forms = FormsOptions {
            enabled: true,
            provider: FormProvider::Custom,
            endpoint: "https://api.example.com/forms".into(),
            ids: HashMap::new(),
        };
        wire_forms_in_document(&mut doc, &forms, None).unwrap();
        let html = crate::parse::serialize_document(&doc);
        assert!(html.contains(r#"action="https://api.example.com/forms""#));
    }

    #[test]
    fn unmapped_form_errors() {
        let mut doc = crate::parse::parse_document(
            r#"<form name="missing" statica></form>"#,
        )
        .unwrap();
        let err = wire_forms_in_document(&mut doc, &formspree(&[]), None).unwrap_err();
        assert!(err.to_string().contains("[forms.ids]"));
    }

    #[test]
    fn disabled_skips_wiring() {
        let mut doc = crate::parse::parse_document(
            r#"<form name="contact" statica></form>"#,
        )
        .unwrap();
        wire_forms_in_document(&mut doc, &FormsOptions::default(), None).unwrap();
        let html = crate::parse::serialize_document(&doc);
        assert!(html.contains("statica"));
        assert!(!html.contains("formspree.io"));
    }
}
