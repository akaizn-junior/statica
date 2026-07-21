//! Content ingestion for funnel sources — JSON, JS literals, Markdown, directories, globs.

mod markdown;

use std::fs;
use std::path::{Component, Path, PathBuf};

use glob::glob;
use serde_json::Value;

use crate::aliases;
use crate::error::{Error, Result};
use crate::funnel::parse_js_value;

/// Load a funnel content source into a `serde_json::Value`.
///
/// Supports:
/// - `.json` / `.js` / `.mjs` — JS value literals (JSON-compatible)
/// - `.md` / `.markdown` — YAML frontmatter + Markdown body → object with `html`
/// - directories — all content files in the directory (non-recursive), as an array
/// - glob patterns in `src` (e.g. `../content/posts/*.md`) — matched files as an array
pub fn load_content(site_root: &Path, page_dir: &Path, src: &str) -> Result<Value> {
    if src.contains('*') || src.contains('?') {
        return load_glob(site_root, page_dir, src);
    }

    let path = resolve_path(site_root, page_dir, src)?;
    if path.is_dir() {
        return load_dir(&path);
    }
    load_file(&path)
}

fn is_content_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| matches!(ext, "json" | "js" | "mjs" | "md" | "markdown"))
}

fn load_file(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).map_err(|e| Error::read(path.display().to_string(), e))?;
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
        "md" | "markdown" => markdown::parse_markdown_file(&text, path),
        _ => parse_js_value(&text)
            .map_err(|message| Error::invalid_js_value(path.display().to_string(), message)),
    }
}

fn load_dir(dir: &Path) -> Result<Value> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| Error::read(dir.display().to_string(), e))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| is_content_file(p))
        .collect();
    paths.sort();
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        items.push(load_file(&path)?);
    }
    Ok(Value::Array(items))
}

fn load_glob(site_root: &Path, page_dir: &Path, pattern: &str) -> Result<Value> {
    let pattern_path = if Path::new(pattern).is_absolute() {
        PathBuf::from(pattern)
    } else if let Some(rest) = pattern.strip_prefix("./") {
        if rest.contains('/') {
            site_root.join(rest)
        } else {
            page_dir.join(pattern)
        }
    } else {
        page_dir.join(pattern)
    };
    let pattern = normalize(&pattern_path).to_string_lossy().to_string();
    let mut paths: Vec<PathBuf> = glob(&pattern)
        .map_err(|e| Error::invalid_content(&pattern, e.to_string()))?
        .filter_map(|entry| entry.ok())
        .filter(|p| is_content_file(p))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(Error::invalid_content(
            &pattern,
            "glob matched no content files",
        ));
    }
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        items.push(load_file(&path)?);
    }
    Ok(Value::Array(items))
}

fn resolve_path(site_root: &Path, page_dir: &Path, rel: &str) -> Result<PathBuf> {
    let joined = aliases::resolve_local_href(site_root, page_dir, rel);
    if let Ok(canon) = joined.canonicalize() {
        return Ok(canon);
    }
    let normalized = normalize(&joined);
    if normalized.exists() {
        return Ok(normalized);
    }
    if joined.exists() {
        return Ok(joined);
    }
    Err(Error::invalid_content(
        joined.display().to_string(),
        "path not found",
    ))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "statica-content-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn loads_json_array() {
        let dir = temp_dir();
        fs::write(
            dir.join("posts.json"),
            r#"[{"slug":"a","headline":"A"}]"#,
        )
        .unwrap();
        let value = load_content(&dir, &dir, "posts.json").unwrap();
        assert_eq!(value, json!([{"slug": "a", "headline": "A"}]));
    }

    #[test]
    fn loads_markdown_with_frontmatter() {
        let dir = temp_dir();
        fs::write(
            dir.join("hello-world.md"),
            r#"---
slug: hello-world
headline: Hello world
published_at: 2026-07-01
summary: First post
---

Build stamps this into **static HTML**.
"#,
        )
        .unwrap();
        let value = load_content(&dir, &dir, "hello-world.md").unwrap();
        let obj = value.as_object().unwrap();
        assert_eq!(obj["slug"], "hello-world");
        assert_eq!(obj["headline"], "Hello world");
        assert!(obj["html"].as_str().unwrap().contains("<strong>static HTML</strong>"));
    }

    #[test]
    fn markdown_slug_defaults_to_filename() {
        let dir = temp_dir();
        fs::write(dir.join("my-post.md"), "# Title\n").unwrap();
        let value = load_content(&dir, &dir, "my-post.md").unwrap();
        assert_eq!(value["slug"], "my-post");
        assert!(value["html"].as_str().unwrap().contains("<h1>Title</h1>"));
    }

    #[test]
    fn loads_content_directory_as_array() {
        let dir = temp_dir();
        let posts = dir.join("posts");
        fs::create_dir_all(&posts).unwrap();
        fs::write(
            posts.join("b.md"),
            "---\nslug: b\nheadline: B\n---\n\nBody B.",
        )
        .unwrap();
        fs::write(
            posts.join("a.md"),
            "---\nslug: a\nheadline: A\n---\n\nBody A.",
        )
        .unwrap();
        let value = load_content(&dir, &dir, "posts").unwrap();
        let arr = value.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["slug"], "a");
        assert_eq!(arr[1]["slug"], "b");
    }

    #[test]
    fn loads_glob_pattern() {
        let dir = temp_dir();
        let posts = dir.join("posts");
        fs::create_dir_all(&posts).unwrap();
        fs::write(
            posts.join("one.md"),
            "---\nslug: one\nheadline: One\n---\n\nOne.",
        )
        .unwrap();
        fs::write(
            posts.join("two.md"),
            "---\nslug: two\nheadline: Two\n---\n\nTwo.",
        )
        .unwrap();
        let pattern = posts.join("*.md").to_string_lossy().to_string();
        let value = load_content(&dir, &dir, &pattern).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 2);
    }
}
