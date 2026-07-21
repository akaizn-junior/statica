//! Markdown content with optional YAML frontmatter.

use std::path::Path;

use pulldown_cmark::{html, Options, Parser};
use serde_json::{Map, Value};

use crate::error::{Error, Result};

/// Convert Markdown to HTML (CommonMark + tables + strikethrough).
pub fn markdown_to_html(source: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(source, options);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// Parse a Markdown file: YAML frontmatter fields + `html` from the body.
pub fn parse_markdown_file(source: &str, path: &Path) -> Result<Value> {
    let (frontmatter, body) = split_frontmatter(source);
    let mut map: Map<String, Value> = if let Some(fm) = frontmatter {
        serde_yaml::from_str(fm).map_err(|e| {
            Error::invalid_content(path.display().to_string(), format!("invalid frontmatter: {e}"))
        })?
    } else {
        Map::new()
    };

    if !map.contains_key("slug") {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            map.insert("slug".into(), Value::String(stem.to_string()));
        }
    }

    if !map.contains_key("html") {
        map.insert("html".into(), Value::String(markdown_to_html(body)));
    }

    Ok(Value::Object(map))
}

fn split_frontmatter(source: &str) -> (Option<&str>, &str) {
    let trimmed = source.trim_start();
    if !trimmed.starts_with("---") {
        return (None, source);
    }
    let rest = trimmed.strip_prefix("---").unwrap_or(trimmed);
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    if let Some(end) = rest.find("\n---") {
        let fm = rest[..end].trim();
        let body = rest[end + 4..].strip_prefix('\n').unwrap_or(&rest[end + 4..]).trim_start();
        return (Some(fm), body);
    }
    (None, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn splits_yaml_frontmatter() {
        let (fm, body) = split_frontmatter(
            "---\nslug: a\n---\n\nHello **world**.",
        );
        assert_eq!(fm, Some("slug: a"));
        assert_eq!(body, "Hello **world**.");
    }

    #[test]
    fn no_frontmatter_returns_full_body() {
        let (fm, body) = split_frontmatter("# Hi\n");
        assert!(fm.is_none());
        assert_eq!(body, "# Hi\n");
    }

    #[test]
    fn renders_markdown_headings() {
        let html = markdown_to_html("## Subtitle\n\nParagraph.");
        assert!(html.contains("<h2>Subtitle</h2>"));
        assert!(html.contains("<p>Paragraph.</p>"));
    }

    #[test]
    fn parse_adds_html_from_body() {
        let value = parse_markdown_file(
            "---\ntitle: Test\n---\n\n**bold**",
            Path::new("test.md"),
        )
        .unwrap();
        assert_eq!(value["title"], "Test");
        assert_eq!(value["slug"], "test");
        assert!(value["html"].as_str().unwrap().contains("<strong>bold</strong>"));
    }
}
