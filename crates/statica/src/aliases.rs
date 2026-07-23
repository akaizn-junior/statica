//! Path / URL aliases for authoring (`@Name/tail` → resolved path or URL).
//!
//! Aliases are defined in `statica.toml` under `[aliases.paths]` (local) and
//! `[aliases.urls]` (URLs). Use the configured symbol (default `@`) plus a
//! `/`-separated tail — e.g. `@Google/?family=Outfit&display=swap`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::parse::{Document, Node};

/// Attributes that may contain alias paths (resolved at build time).
const PATH_ATTRS: &[&str] = &["href", "src", "poster", "action"];

/// Alias map from project config (`[aliases]` in statica.toml).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasOptions {
    /// Leading symbol for alias references (default `@`).
    pub symbol: String,
    /// Name → local path prefix (`[aliases.paths]`).
    pub paths: HashMap<String, String>,
    /// Name → URL prefix (`[aliases.urls]`).
    pub urls: HashMap<String, String>,
}

impl Default for AliasOptions {
    fn default() -> Self {
        let mut urls = HashMap::new();
        urls.insert(
            "Google".into(),
            "https://fonts.googleapis.com/css2".into(),
        );
        Self {
            symbol: "@".into(),
            paths: HashMap::new(),
            urls,
        }
    }
}

impl AliasOptions {
    fn lookup(&self, name: &str) -> Option<&str> {
        self.paths
            .get(name)
            .or_else(|| self.urls.get(name))
            .map(String::as_str)
    }

    fn knows(&self, name: &str) -> bool {
        self.paths.contains_key(name) || self.urls.contains_key(name)
    }

    /// Parse `@Name/tail` when `value` starts with [`Self::symbol`].
    #[must_use]
    pub fn parse<'a>(&'a self, value: &'a str) -> Option<ResolvedAlias<'a>> {
        let rest = value.trim().strip_prefix(&self.symbol)?;
        let (name, tail) = match rest.find('/') {
            Some(i) => {
                let name = &rest[..i];
                let tail = rest[i + 1..].trim_start_matches('/');
                (name, tail)
            }
            None => (rest, ""),
        };
        if name.is_empty() {
            return None;
        }
        let base = self.lookup(name)?;
        Some(ResolvedAlias { base, tail })
    }

    /// True when `value` starts with the alias symbol (may still be invalid / unknown).
    #[must_use]
    pub fn looks_like_alias(&self, value: &str) -> bool {
        value.trim().starts_with(&self.symbol)
    }

    /// Unknown alias name from a symbol-prefixed value (for diagnostics).
    #[must_use]
    pub fn unknown_alias_name<'a>(&'a self, value: &'a str) -> Option<&'a str> {
        let rest = value.trim().strip_prefix(&self.symbol)?;
        let name = rest.split('/').next().unwrap_or("");
        if name.is_empty() || self.knows(name) {
            None
        } else {
            Some(name)
        }
    }
}

/// An alias reference before joining base + tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedAlias<'a> {
    pub base: &'a str,
    pub tail: &'a str,
}

/// Resolve alias paths in every [`PATH_ATTRS`] on all elements.
pub fn resolve_paths_in_document(
    doc: &mut Document,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<()> {
    resolve_paths_in_nodes(&mut doc.children, aliases, site)
}

pub fn resolve_paths_in_nodes(
    nodes: &mut [Node],
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
) -> Result<()> {
    for node in nodes {
        if let Node::Element(el) = node {
            for attr in PATH_ATTRS {
                if let Some(raw) = el.attrs.get(*attr).cloned() {
                    let resolved = resolve_path(&raw, aliases, site, attr)?;
                    if resolved != raw {
                        el.attrs.insert(attr.to_string(), resolved);
                    }
                }
            }
            resolve_paths_in_nodes(&mut el.children, aliases, site)?;
        }
    }
    Ok(())
}

/// Resolve a single path/URL value. Non-alias values pass through unchanged.
pub fn resolve_path(
    value: &str,
    aliases: &AliasOptions,
    site: Option<(&str, &str)>,
    attr: &str,
) -> Result<String> {
    if !aliases.looks_like_alias(value) {
        return Ok(value.to_string());
    }
    if let Some(r) = aliases.parse(value) {
        return Ok(join_alias(r.base, r.tail));
    }
    if let Some(name) = aliases.unknown_alias_name(value) {
        return Err(alias_err(
            site,
            value,
            attr,
            format!(
                "unknown alias `{}{name}` (define it under [aliases.paths] or [aliases.urls] in statica.toml)",
                aliases.symbol
            ),
        ));
    }
    Err(alias_err(
        site,
        value,
        attr,
        format!(
            "invalid alias path `{value}` (expected `{symbol}Name/path` or `{symbol}Name/?query`)",
            symbol = aliases.symbol
        ),
    ))
}

/// Join alias base and tail into a final URL or local path.
#[must_use]
pub fn join_alias(base: &str, tail: &str) -> String {
    if base.starts_with("http://") || base.starts_with("https://") {
        if tail.is_empty() {
            return base.to_string();
        }
        if tail.starts_with('?') {
            return format!("{base}{tail}");
        }
        if base.contains('?') {
            format!("{base}&{tail}")
        } else {
            format!("{base}?{tail}")
        }
    } else {
        let base = base.trim_end_matches('/');
        let tail = tail.trim_start_matches('/');
        if tail.is_empty() {
            base.to_string()
        } else {
            format!("{base}/{tail}")
        }
    }
}

/// Resolve a local href/src after alias expansion.
///
/// Paths from `[aliases.paths]` use a `./dir/…` prefix and are site-root-relative.
/// `./file.ext` (no slash) resolves from `page_dir` (sibling fragment imports).
/// Other relative paths resolve from `page_dir` (e.g. `../ui/foo.html`).
#[must_use]
pub fn resolve_local_href(site_root: &Path, page_dir: &Path, rel: &str) -> PathBuf {
    if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else if let Some(rest) = rel.strip_prefix("./") {
        if rest.contains('/') {
            site_root.join(rest)
        } else {
            page_dir.join(rel)
        }
    } else if rel.starts_with("../") {
        page_dir.join(rel)
    } else {
        site_root.join(rel)
    }
}

/// Google Fonts css2 URLs benefit from preconnect hints (once per page).
#[must_use]
pub fn is_google_fonts_css(url: &str) -> bool {
    url.starts_with("https://fonts.googleapis.com/")
        || url.starts_with("http://fonts.googleapis.com/")
}

fn alias_err(
    site: Option<(&str, &str)>,
    value: &str,
    attr: &str,
    message: impl Into<String>,
) -> Error {
    let dq = format!("{attr}=\"{value}\"");
    let sq = format!("{attr}='{value}'");
    match site {
        Some((file, source)) => Error::at(file, source, &[dq.as_str(), sq.as_str()], message),
        None => Error::at_file("<page>", message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn resolves_google_alias_with_query_path() {
        let aliases = AliasOptions::default();
        let r = aliases
            .parse("@Google/?family=Outfit:wght@100..900&display=swap")
            .unwrap();
        assert_eq!(r.base, "https://fonts.googleapis.com/css2");
        assert_eq!(r.tail, "?family=Outfit:wght@100..900&display=swap");
        assert_eq!(
            join_alias(r.base, r.tail),
            "https://fonts.googleapis.com/css2?family=Outfit:wght@100..900&display=swap"
        );
    }

    #[test]
    fn resolve_local_href_alias_paths() {
        let site = Path::new("/site");
        let page = Path::new("/site/[locale]");
        assert_eq!(
            resolve_local_href(site, page, "ui/button.html"),
            PathBuf::from("/site/ui/button.html")
        );
        assert_eq!(
            resolve_local_href(site, page, "./post-card.html"),
            PathBuf::from("/site/[locale]/post-card.html")
        );
        let mut paths = std::collections::HashMap::new();
        paths.insert("ui".into(), "ui".into());
        let aliases = AliasOptions {
            symbol: "@".into(),
            paths,
            urls: HashMap::new(),
        };
        let resolved = resolve_path("@ui/button.html", &aliases, None, "href").unwrap();
        assert_eq!(resolved, "ui/button.html");
        assert_eq!(
            resolve_local_href(site, page, &resolved),
            PathBuf::from("/site/ui/button.html")
        );
    }

    #[test]
    fn resolves_google_alias_without_leading_question() {
        let aliases = AliasOptions::default();
        let r = aliases
            .parse("@Google/family=Outfit:wght@400&display=swap")
            .unwrap();
        assert_eq!(
            join_alias(r.base, r.tail),
            "https://fonts.googleapis.com/css2?family=Outfit:wght@400&display=swap"
        );
    }

    #[test]
    fn joins_local_alias_paths() {
        let mut aliases = AliasOptions::default();
        aliases
            .paths
            .insert("fonts".into(), "./assets/fonts".into());
        let r = aliases.parse("@fonts/outfit.css").unwrap();
        assert_eq!(join_alias(r.base, r.tail), "./assets/fonts/outfit.css");
    }

    #[test]
    fn resolve_path_passes_through_plain_paths() {
        let aliases = AliasOptions::default();
        assert_eq!(
            resolve_path("./app.js", &aliases, None, "src").unwrap(),
            "./app.js"
        );
    }

    #[test]
    fn resolve_paths_in_document_rewrites_src_and_href() {
        let mut doc = crate::parse::parse_document(
            r#"<!doctype html><html><body>
<a href="@Google/?family=Outfit&display=swap">x</a>
<script src="@assets/app.js"></script>
</body></html>"#,
        )
        .unwrap();
        let mut aliases = AliasOptions::default();
        aliases.paths.insert("assets".into(), "./static".into());

        resolve_paths_in_document(&mut doc, &aliases, None).unwrap();
        let html = crate::parse::serialize_document(&doc);
        assert!(html.contains("fonts.googleapis.com/css2?family=Outfit"));
        assert!(html.contains(r#"src="./static/app.js""#));
    }
}
