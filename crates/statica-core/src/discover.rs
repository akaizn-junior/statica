use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct PageSource {
    /// Absolute path to index.html
    pub path: PathBuf,
    /// Route relative to project root, using `/` (e.g. `posts/[slug]` or `` for home)
    pub route: String,
    /// Dynamic param names in order, e.g. ["slug"]
    pub params: Vec<String>,
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
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() != "index.html" {
            continue;
        }
        let path = entry.path().to_path_buf();
        let parent = path
            .parent()
            .ok_or_else(|| Error::msg(format!("orphan index.html at {}", path.display())))?;
        let rel = parent
            .strip_prefix(root)
            .map_err(|_| Error::msg(format!("path outside root: {}", path.display())))?;
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
            path,
            route,
            params,
        });
    }
    pages.sort_by(|a, b| a.route.cmp(&b.route));
    Ok(pages)
}

fn parse_params(route: &str) -> Vec<String> {
    route
        .split('/')
        .filter_map(|seg| {
            let seg = seg.trim();
            if seg.starts_with('[') && seg.ends_with(']') && seg.len() > 2 {
                Some(seg[1..seg.len() - 1].to_string())
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
        assert_eq!(parse_params("posts/[slug]"), vec!["slug".to_string()]);
        assert!(parse_params("blog").is_empty());
    }
}
