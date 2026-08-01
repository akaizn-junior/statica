//! statica `${path}` template token parsing.

use super::bind_decl::is_identifier;

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
}
