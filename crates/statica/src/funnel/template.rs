//! statica `${path}` template token parsing.

use super::bind_decl::is_identifier;
use serde_json::Value;
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DottedPath(String);

impl DottedPath {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() || !raw.split('.').all(is_identifier) {
            return None;
        }
        Some(Self(raw.to_string()))
    }

    #[must_use]
    pub fn root(&self) -> &str {
        self.0.split('.').next().unwrap_or(self.0.as_str())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplatePlaceholder {
    Path(DottedPath),
    Expression(String),
}

impl TemplatePlaceholder {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        Some(if let Some(path) = DottedPath::parse(raw) {
            Self::Path(path)
        } else {
            Self::Expression(raw.to_string())
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateToken<'a> {
    Text(&'a str),
    Placeholder(TemplatePlaceholder),
}

#[must_use]
pub fn has_template_tokens(raw: &str) -> bool {
    raw.contains("${")
}

#[must_use]
pub fn template_tokens(raw: &str) -> Vec<TemplateToken<'_>> {
    let mut out = Vec::new();
    let mut text_start = 0;
    let mut scan_at = 0;

    while let Some(rel_start) = raw[scan_at..].find("${") {
        let start = scan_at + rel_start;
        let expr_start = start + 2;
        let Some(rel_end) = raw[expr_start..].find('}') else {
            break;
        };
        let end = expr_start + rel_end;
        if text_start < start {
            out.push(TemplateToken::Text(&raw[text_start..start]));
        }
        if let Some(placeholder) = TemplatePlaceholder::parse(&raw[expr_start..end]) {
            out.push(TemplateToken::Placeholder(placeholder));
        }
        scan_at = end + 1;
        text_start = scan_at;
    }

    if text_start < raw.len() {
        out.push(TemplateToken::Text(&raw[text_start..]));
    }

    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicAttributeError {
    Expression(String),
    MissingPath(String),
    NonScalar(String),
}

impl DynamicAttributeError {
    #[must_use]
    pub fn authored(&self) -> String {
        match self {
            Self::Expression(expr) => format!("${{{expr}}}"),
            Self::MissingPath(path) | Self::NonScalar(path) => format!("${{{path}}}"),
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Expression(expr) => {
                let authored = format!("${{{expr}}}");
                format!("template placeholder `{authored}` must be a dotted identifier path, not a JS expression")
            }
            Self::MissingPath(path) => {
                format!("template placeholder `${{{path}}}` is not defined in this context")
            }
            Self::NonScalar(path) => {
                format!("template placeholder `${{{path}}}` must resolve to a string, number, boolean, or null")
            }
        }
    }
}

pub fn expand_dynamic_attribute(
    raw: &str,
    context: &Value,
) -> std::result::Result<String, DynamicAttributeError> {
    template_tokens(raw)
        .into_iter()
        .map(|token| match token {
            TemplateToken::Text(text) => Ok(Cow::Borrowed(text)),
            TemplateToken::Placeholder(TemplatePlaceholder::Path(path)) => {
                let value = path_value(context, path.as_str())
                    .ok_or_else(|| DynamicAttributeError::MissingPath(path.as_str().to_string()))?;
                value_as_attr_scalar(value, path.as_str()).map(Cow::Owned)
            }
            TemplateToken::Placeholder(TemplatePlaceholder::Expression(expr)) => {
                Err(DynamicAttributeError::Expression(expr))
            }
        })
        .collect()
}

fn path_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let mut cur = value;
    for part in path.split('.').filter(|p| !p.is_empty()) {
        cur = match cur {
            Value::Object(map) => map.get(part)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn value_as_attr_scalar(
    value: &Value,
    path: &str,
) -> std::result::Result<String, DynamicAttributeError> {
    match value {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Array(_) | Value::Object(_) => Err(DynamicAttributeError::NonScalar(path.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_text_paths_and_expressions() {
        assert_eq!(
            template_tokens("Hi ${item.title} ${a + b} ${ }"),
            vec![
                TemplateToken::Text("Hi "),
                TemplateToken::Placeholder(TemplatePlaceholder::Path(DottedPath(
                    "item.title".into()
                ))),
                TemplateToken::Text(" "),
                TemplateToken::Placeholder(TemplatePlaceholder::Expression("a + b".into())),
                TemplateToken::Text(" "),
            ]
        );
    }

    #[test]
    fn expands_dynamic_attribute_from_context() {
        let ctx = serde_json::json!({"i18n": {"locale": "pt"}, "item": {"slug": "ola"}});
        assert_eq!(
            expand_dynamic_attribute("../content/posts.${i18n.locale}/${item.slug}.json", &ctx)
                .unwrap(),
            "../content/posts.pt/ola.json"
        );
    }

    #[test]
    fn dynamic_attribute_rejects_expressions() {
        let err = expand_dynamic_attribute("${a + b}", &serde_json::json!({})).unwrap_err();
        assert_eq!(
            err.message(),
            "template placeholder `${a + b}` must be a dotted identifier path, not a JS expression"
        );
    }

    #[test]
    fn dynamic_attribute_requires_defined_paths() {
        let err = expand_dynamic_attribute("${i18n.locale}", &serde_json::json!({})).unwrap_err();
        assert_eq!(
            err.message(),
            "template placeholder `${i18n.locale}` is not defined in this context"
        );
    }
}
