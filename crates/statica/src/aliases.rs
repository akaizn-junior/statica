//! Path / URL aliases for authoring (`@Name/tail` → resolved path or URL).
//!
//! Aliases are defined in `statica.toml` under `[aliases.paths]` (local) and
//! `[aliases.urls]` (URLs). Use the configured symbol (default `@`) plus a
//! `/`-separated tail — e.g. `@fonts/?family=Outfit&display=swap`.

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
    pub paths: HashMap<String, LocalAlias>,
    /// Name → URL prefix (`[aliases.urls]`).
    pub urls: HashMap<String, UrlAlias>,
}

impl Default for AliasOptions {
    fn default() -> Self {
        Self {
            symbol: "@".into(),
            paths: HashMap::new(),
            urls: HashMap::new(),
        }
    }
}

impl AliasOptions {
    /// Validate alias shape once before build-time resolution begins.
    ///
    /// The CLI performs the same user-facing validation while loading
    /// `statica.toml`; this keeps direct library use honest too.
    pub fn validate(&self) -> Result<()> {
        if self.symbol.trim().is_empty() {
            return Err(Error::at_file(
                "<config>",
                "[aliases].symbol cannot be empty",
            ));
        }
        for (name, alias) in &self.paths {
            validate_alias_name(name)?;
            if alias.base.trim().is_empty() {
                return Err(Error::at_file(
                    "<config>",
                    format!("[aliases.paths].{name} cannot be empty"),
                ));
            }
            if is_url_base(&alias.base) {
                return Err(Error::at_file(
                    "<config>",
                    format!(
                        "[aliases.paths].{name} must be a local path, not a URL (use [aliases.urls] for URLs)"
                    ),
                ));
            }
        }
        for (name, alias) in &self.urls {
            validate_alias_name(name)?;
            if !is_url_base(&alias.base) {
                return Err(Error::at_file(
                    "<config>",
                    format!("[aliases.urls].{name} must be a URL (http:// or https://)"),
                ));
            }
        }
        for name in self.paths.keys() {
            if self.urls.contains_key(name) {
                return Err(Error::at_file(
                    "<config>",
                    format!("alias `{name}` is defined in both [aliases.paths] and [aliases.urls]"),
                ));
            }
        }
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<AliasTarget<'_>> {
        self.paths
            .get(name)
            .map(AliasTarget::Path)
            .or_else(|| self.urls.get(name).map(AliasTarget::Url))
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
        let target = self.lookup(name)?;
        Some(ResolvedAlias { target, tail })
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
    pub target: AliasTarget<'a>,
    pub tail: &'a str,
}

impl ResolvedAlias<'_> {
    #[must_use]
    pub fn base(&self) -> &str {
        self.target.base()
    }
}

/// Typed alias target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasTarget<'a> {
    /// Local path alias from `[aliases.paths]`.
    Path(&'a LocalAlias),
    /// URL alias from `[aliases.urls]`.
    Url(&'a UrlAlias),
}

impl AliasTarget<'_> {
    #[must_use]
    pub fn base(&self) -> &str {
        match self {
            Self::Path(alias) => &alias.base,
            Self::Url(alias) => &alias.base,
        }
    }
}

/// Local alias base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAlias {
    pub base: String,
}

impl LocalAlias {
    #[must_use]
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }
}

/// URL alias base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlAlias {
    pub base: String,
}

impl UrlAlias {
    #[must_use]
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }
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
        return Ok(join_alias(r.base(), r.tail));
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
    if is_url_base(base) {
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

fn validate_alias_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.contains('/') || name.chars().any(char::is_whitespace) {
        return Err(Error::at_file(
            "<config>",
            format!("alias name `{name}` must be non-empty and cannot contain whitespace or `/`"),
        ));
    }
    Ok(())
}

fn is_url_base(base: &str) -> bool {
    base.starts_with("http://") || base.starts_with("https://")
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
    fn resolves_url_alias_with_query_path() {
        let mut aliases = AliasOptions::default();
        aliases.urls.insert(
            "fonts".into(),
            UrlAlias::new("https://fonts.googleapis.com/css2"),
        );
        let r = aliases
            .parse("@fonts/?family=Outfit:wght@100..900&display=swap")
            .unwrap();
        assert_eq!(r.base(), "https://fonts.googleapis.com/css2");
        assert_eq!(r.tail, "?family=Outfit:wght@100..900&display=swap");
        assert_eq!(
            join_alias(r.base(), r.tail),
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
        let aliases = AliasOptions {
            symbol: "@".into(),
            paths: [("ui".into(), LocalAlias::new("ui"))].into(),
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
    fn resolves_url_alias_without_leading_question() {
        let mut aliases = AliasOptions::default();
        aliases.urls.insert(
            "fonts".into(),
            UrlAlias::new("https://fonts.googleapis.com/css2"),
        );
        let r = aliases
            .parse("@fonts/family=Outfit:wght@400&display=swap")
            .unwrap();
        assert_eq!(
            join_alias(r.base(), r.tail),
            "https://fonts.googleapis.com/css2?family=Outfit:wght@400&display=swap"
        );
    }

    #[test]
    fn joins_local_alias_paths() {
        let mut aliases = AliasOptions::default();
        aliases
            .paths
            .insert("fonts".into(), LocalAlias::new("./assets/fonts"));
        let r = aliases.parse("@fonts/outfit.css").unwrap();
        assert_eq!(join_alias(r.base(), r.tail), "./assets/fonts/outfit.css");
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
<a href="@fonts/?family=Outfit&display=swap">x</a>
<script src="@assets/app.js"></script>
</body></html>"#,
        )
        .unwrap();
        let mut aliases = AliasOptions::default();
        aliases.urls.insert(
            "fonts".into(),
            UrlAlias::new("https://fonts.googleapis.com/css2"),
        );
        aliases
            .paths
            .insert("assets".into(), LocalAlias::new("./static"));

        resolve_paths_in_document(&mut doc, &aliases, None).unwrap();
        let html = crate::parse::serialize_document(&doc);
        assert!(html.contains("fonts.googleapis.com/css2?family=Outfit"));
        assert!(html.contains(r#"src="./static/app.js""#));
    }
}
