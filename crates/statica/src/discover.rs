use std::fmt;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::{Error, Result};

/// Absolute source file for one statica page (`**/index.html`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PageFile(PathBuf);

impl PageFile {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Filesystem route for a page, relative to the site root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PageRoute(String);

impl PageRoute {
    #[must_use]
    pub fn new(route: String) -> Self {
        Self(route)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_home(&self) -> bool {
        self.0.is_empty()
    }
}

/// Dynamic route segment without brackets, such as `slug`, `page`, or `locale`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteParam(String);

impl RouteParam {
    #[must_use]
    pub fn new(param: String) -> Self {
        Self(param)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RouteParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct PageSource {
    pub path: PageFile,
    pub route: PageRoute,
    pub params: Vec<RouteParam>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Static,
    Collection,
}

impl PageSource {
    #[must_use]
    pub fn kind(&self) -> PageKind {
        if self.params.is_empty() {
            PageKind::Static
        } else {
            PageKind::Collection
        }
    }
}

/// Discover every `**/index.html` under `root`.
pub fn discover_pages(root: &Path, ignore_dirs: &[String]) -> Result<Vec<PageSource>> {
    let mut pages = Vec::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            if name.starts_with('.') {
                return false;
            }
            !ignore_dirs.iter().any(|d| d == name.as_ref())
        })
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() != "index.html" {
            continue;
        }
        let path = entry.path().to_path_buf();
        let parent = path.parent().ok_or_else(|| {
            Error::at_file(
                path.display().to_string(),
                format!("orphan index.html at {}", path.display()),
            )
        })?;
        let rel = parent.strip_prefix(root).map_err(|_| {
            Error::at_file(
                path.display().to_string(),
                format!("path outside root: {}", path.display()),
            )
        })?;
        let route = if rel.as_os_str().is_empty() {
            String::new()
        } else {
            rel.iter()
                .map(|c| c.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        };
        let params = parse_params(&route);
        pages.push(PageSource {
            path: PageFile::new(path),
            route: PageRoute::new(route),
            params,
        });
    }
    pages.sort_by(|a, b| a.route.cmp(&b.route));
    Ok(pages)
}

fn parse_params(route: &str) -> Vec<RouteParam> {
    route
        .split('/')
        .filter_map(|seg| {
            let seg = seg.trim();
            if seg.starts_with('[') && seg.ends_with(']') && seg.len() > 2 {
                Some(RouteParam::new(seg[1..seg.len() - 1].to_string()))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_slug_param() {
        assert_eq!(parse_params("posts/[slug]")[0].as_str(), "slug");
        assert!(parse_params("blog").is_empty());
    }
}
