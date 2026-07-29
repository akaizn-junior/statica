//! Content ingestion for funnel sources — JSON, JSONL, CSV, text, Markdown, and globs.

mod markdown;

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Component, Path, PathBuf};

use globwalk::GlobWalkerBuilder;
use rayon::prelude::*;
use serde_json::Value;

use crate::aliases;
use crate::error::{Error, Result};

/// Parsed funnel content with the source kind statica used to read it.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedContent {
    pub kind: DataKind,
    pub data: DataSet,
}

/// Source-shaped data. statica projects this to JSON only when binding needs it.
#[derive(Debug, Clone, PartialEq)]
pub enum DataSet {
    Json(Value),
    Records(Vec<Value>),
    Lines(Vec<String>),
    Markdown(Value),
    Glob(Vec<DataSet>),
}

impl DataSet {
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Json(value) | Self::Markdown(value) => value.clone(),
            Self::Records(records) => Value::Array(records.clone()),
            Self::Lines(lines) => Value::Array(lines.iter().cloned().map(Value::String).collect()),
            Self::Glob(items) => Value::Array(items.iter().map(Self::to_value).collect()),
        }
    }

    #[must_use]
    pub fn as_array(&self) -> Option<Vec<Value>> {
        match self {
            Self::Json(Value::Array(items)) | Self::Records(items) => Some(items.clone()),
            Self::Lines(lines) => Some(lines.iter().cloned().map(Value::String).collect()),
            Self::Glob(items) => Some(items.iter().map(Self::to_value).collect()),
            Self::Json(_) | Self::Markdown(_) => None,
        }
    }
}

/// Data source formats statica can infer from a path or accept from a `type` attr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Json,
    Jsonl,
    Csv,
    Text,
    Markdown,
    Glob,
}

impl DataKind {
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_extension)
    }

    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "json" => Some(Self::Json),
            "jsonl" | "ndjson" => Some(Self::Jsonl),
            "csv" => Some(Self::Csv),
            "txt" | "text" => Some(Self::Text),
            "md" | "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }

    #[must_use]
    pub fn from_type_attr(raw: &str) -> Option<Self> {
        let raw = raw.trim().to_ascii_lowercase();
        let mime = raw
            .split_once(';')
            .map_or(raw.as_str(), |(mime, _)| mime.trim());
        match mime {
            "json" | "application/json" | "text/json" => Some(Self::Json),
            "jsonl"
            | "ndjson"
            | "application/jsonl"
            | "application/x-jsonlines"
            | "application/x-ndjson"
            | "application/ndjson" => Some(Self::Jsonl),
            "csv" | "text/csv" | "application/csv" => Some(Self::Csv),
            "text" | "plain" | "text/plain" => Some(Self::Text),
            "md" | "markdown" | "text/markdown" | "text/x-markdown" => Some(Self::Markdown),
            _ => None,
        }
    }
}

/// Load a funnel content source into source-shaped data.
///
/// Supports:
/// - `.json` — JSON values
/// - `.jsonl` / `.ndjson` — one JSON value per non-empty line, as an array
/// - `.csv` — header row + records, as an array of objects
/// - `.txt` / `.text` — one string per line, as an array
/// - `.md` / `.markdown` — YAML frontmatter + Markdown body → object with `html`
/// - glob patterns in `href` (e.g. `../content/posts/*.md`) — matched files as an array
pub fn load_content(
    site_root: &Path,
    page_dir: &Path,
    src: &str,
    explicit_kind: Option<DataKind>,
) -> Result<LoadedContent> {
    if is_glob_href(src) {
        return load_glob(site_root, page_dir, src, explicit_kind);
    }

    let path = resolve_path(site_root, page_dir, src)?;
    if path.is_dir() {
        return Err(Error::invalid_content(
            path.display().to_string(),
            "data href must point to a file or glob, not a directory",
        ));
    }
    let data = load_file(&path, explicit_kind)?;
    Ok(LoadedContent {
        kind: explicit_kind.unwrap_or_else(|| DataKind::from_path(&path).unwrap_or(DataKind::Json)),
        data,
    })
}

fn is_content_file(path: &Path, explicit_kind: Option<DataKind>) -> bool {
    path.is_file() && (explicit_kind.is_some() || DataKind::from_path(path).is_some())
}

fn load_file(path: &Path, explicit_kind: Option<DataKind>) -> Result<DataSet> {
    let kind = explicit_kind
        .or_else(|| DataKind::from_path(path))
        .ok_or_else(|| {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            Error::invalid_content(
                path.display().to_string(),
                format!("unsupported data file extension `.{ext}`"),
            )
        })?;
    match kind {
        DataKind::Json => serde_json::from_reader(open_reader(path)?)
            .map(DataSet::Json)
            .map_err(|e| Error::invalid_content(path.display().to_string(), e.to_string())),
        DataKind::Jsonl => parse_jsonl(open_reader(path)?, path),
        DataKind::Csv => parse_csv(open_file(path)?, path),
        DataKind::Text => parse_text_lines(open_reader(path)?, path),
        DataKind::Markdown => {
            let text =
                fs::read_to_string(path).map_err(|e| Error::read(path.display().to_string(), e))?;
            markdown::parse_markdown_file(&text, path).map(DataSet::Markdown)
        }
        DataKind::Glob => Err(Error::invalid_content(
            path.display().to_string(),
            "glob is a source pattern, not a file type",
        )),
    }
}

fn open_file(path: &Path) -> Result<File> {
    File::open(path).map_err(|e| Error::read(path.display().to_string(), e))
}

fn open_reader(path: &Path) -> Result<BufReader<File>> {
    open_file(path).map(BufReader::new)
}

fn parse_jsonl(source: impl BufRead, path: &Path) -> Result<DataSet> {
    let mut values = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        let line = line.map_err(|e| Error::read(path.display().to_string(), e))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value = serde_json::from_str(line).map_err(|e| {
            Error::invalid_content(
                path.display().to_string(),
                format!("line {} is not valid JSON: {e}", idx + 1),
            )
        })?;
        values.push(value);
    }
    Ok(DataSet::Records(values))
}

fn parse_text_lines(source: impl BufRead, path: &Path) -> Result<DataSet> {
    source
        .lines()
        .map(|line| line.map_err(|e| Error::read(path.display().to_string(), e)))
        .filter_map(|line| match line {
            Ok(line) => {
                let line = line.trim();
                (!line.is_empty()).then(|| Ok(line.to_string()))
            }
            Err(err) => Some(Err(err)),
        })
        .collect::<Result<Vec<_>>>()
        .map(DataSet::Lines)
}

fn parse_csv(source: impl io::Read, path: &Path) -> Result<DataSet> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(source);
    let headers = reader
        .headers()
        .map_err(|e| Error::invalid_content(path.display().to_string(), e.to_string()))?
        .clone();
    validate_csv_headers(&headers, path)?;

    reader
        .records()
        .map(|record| {
            let record = record
                .map_err(|e| Error::invalid_content(path.display().to_string(), e.to_string()))?;
            Ok(Value::Object(
                headers
                    .iter()
                    .zip(record.iter())
                    .map(|(header, value)| (header.to_string(), Value::String(value.to_string())))
                    .collect(),
            ))
        })
        .collect::<Result<Vec<_>>>()
        .map(DataSet::Records)
}

fn validate_csv_headers(headers: &csv::StringRecord, path: &Path) -> Result<()> {
    if headers.is_empty() {
        return Err(Error::invalid_content(
            path.display().to_string(),
            "CSV data needs a header row",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for header in headers {
        if header.is_empty() {
            return Err(Error::invalid_content(
                path.display().to_string(),
                "CSV headers cannot be empty",
            ));
        }
        if !seen.insert(header) {
            return Err(Error::invalid_content(
                path.display().to_string(),
                format!("duplicate CSV header `{header}`"),
            ));
        }
    }
    Ok(())
}

fn load_glob(
    site_root: &Path,
    page_dir: &Path,
    pattern: &str,
    explicit_kind: Option<DataKind>,
) -> Result<LoadedContent> {
    let (base, relative_pattern) = glob_base_and_pattern(site_root, page_dir, pattern);
    let pattern_display = base.join(&relative_pattern).display().to_string();
    let walker = GlobWalkerBuilder::from_patterns(&base, &[relative_pattern.as_str()])
        .file_type(globwalk::FileType::FILE)
        .build()
        .map_err(|e| Error::invalid_content(&pattern_display, e.to_string()))?;
    let mut paths: Vec<PathBuf> = walker
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| is_content_file(path, explicit_kind))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(Error::invalid_content(
            &pattern_display,
            "glob matched no content files",
        ));
    }
    let items = if paths.len() >= 2_000 {
        paths
            .par_iter()
            .map(|path| load_file(path, explicit_kind))
            .collect::<Result<Vec<_>>>()?
    } else {
        paths
            .iter()
            .map(|path| load_file(path, explicit_kind))
            .collect::<Result<Vec<_>>>()?
    };
    Ok(LoadedContent {
        kind: DataKind::Glob,
        data: DataSet::Glob(items),
    })
}

#[must_use]
pub fn is_glob_href(href: &str) -> bool {
    href.contains('*') || href.contains('?') || href.contains('[') || href.contains('{')
}

fn glob_base_and_pattern(site_root: &Path, page_dir: &Path, pattern: &str) -> (PathBuf, String) {
    let path = Path::new(pattern);
    let mut base_prefix = PathBuf::new();
    let mut pattern_parts = Vec::new();
    let mut in_pattern = false;

    for component in path.components() {
        let part = component.as_os_str().to_string_lossy();
        if !in_pattern && !has_glob_meta(&part) {
            base_prefix.push(component.as_os_str());
        } else {
            in_pattern = true;
            pattern_parts.push(part.into_owned());
        }
    }

    let base = if base_prefix.as_os_str().is_empty() {
        page_dir.to_path_buf()
    } else if base_prefix.is_absolute() {
        base_prefix
    } else {
        let prefix = base_prefix.to_string_lossy();
        if let Some(rest) = prefix.strip_prefix("./") {
            if rest.contains('/') {
                site_root.join(rest)
            } else {
                page_dir.join(prefix.as_ref())
            }
        } else {
            aliases::resolve_local_href(site_root, page_dir, prefix.as_ref())
        }
    };
    (normalize(&base), pattern_parts.join("/"))
}

fn has_glob_meta(part: &str) -> bool {
    part.contains('*') || part.contains('?') || part.contains('[') || part.contains('{')
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
        fs::write(dir.join("posts.json"), r#"[{"slug":"a","headline":"A"}]"#).unwrap();
        let loaded = load_content(&dir, &dir, "posts.json", None).unwrap();
        assert_eq!(loaded.kind, DataKind::Json);
        assert_eq!(
            loaded.data.to_value(),
            json!([{"slug": "a", "headline": "A"}])
        );
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
        let value = load_content(&dir, &dir, "hello-world.md", None)
            .unwrap()
            .data
            .to_value();
        let obj = value.as_object().unwrap();
        assert_eq!(obj["slug"], "hello-world");
        assert_eq!(obj["headline"], "Hello world");
        assert!(obj["html"]
            .as_str()
            .unwrap()
            .contains("<strong>static HTML</strong>"));
    }

    #[test]
    fn markdown_slug_defaults_to_filename() {
        let dir = temp_dir();
        fs::write(dir.join("my-post.md"), "# Title\n").unwrap();
        let value = load_content(&dir, &dir, "my-post.md", None)
            .unwrap()
            .data
            .to_value();
        assert_eq!(value["slug"], "my-post");
        assert!(value["html"].as_str().unwrap().contains("<h1>Title</h1>"));
    }

    #[test]
    fn rejects_content_directory() {
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
        let err = load_content(&dir, &dir, "posts", None).unwrap_err();
        assert!(err.to_string().contains("file or glob"));
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
        let loaded = load_content(&dir, &dir, &pattern, None).unwrap();
        assert_eq!(loaded.kind, DataKind::Glob);
        assert_eq!(loaded.data.to_value().as_array().unwrap().len(), 2);
    }

    #[test]
    fn data_markdown_allows_content_without_frontmatter() {
        let dir = temp_dir();
        fs::write(dir.join("post.data.md"), "# Title\n").unwrap();
        let value = load_content(&dir, &dir, "post.data.md", None)
            .unwrap()
            .data
            .to_value();
        assert_eq!(value["slug"], "post.data");
        assert!(value["html"].as_str().unwrap().contains("<h1>Title</h1>"));

        fs::write(dir.join("ok.data.md"), "---\ntitle: Ok\n---\n\nBody.").unwrap();
        let value = load_content(&dir, &dir, "ok.data.md", None)
            .unwrap()
            .data
            .to_value();
        assert_eq!(value["title"], "Ok");
    }

    #[test]
    fn json_requires_valid_json() {
        let dir = temp_dir();
        fs::write(dir.join("posts.data.json"), r#"[{ slug: "a" }]"#).unwrap();
        let err = load_content(&dir, &dir, "posts.data.json", None).unwrap_err();
        assert!(err.to_string().contains("key must be a string"));
    }

    #[test]
    fn rejects_js_data_sources() {
        let dir = temp_dir();
        fs::write(dir.join("posts.data.js"), r#"export default [];"#).unwrap();
        let err = load_content(&dir, &dir, "posts.data.js", None).unwrap_err();
        assert!(err.to_string().contains("unsupported data file extension"));
    }

    #[test]
    fn loads_plain_text_as_lines() {
        let dir = temp_dir();
        fs::write(dir.join("note.txt"), "hello\n\nworld\n").unwrap();
        let loaded = load_content(&dir, &dir, "note.txt", None).unwrap();
        assert_eq!(loaded.kind, DataKind::Text);
        assert_eq!(loaded.data.to_value(), json!(["hello", "world"]));
    }

    #[test]
    fn explicit_type_parses_extensionless_text() {
        let dir = temp_dir();
        fs::write(dir.join("notes"), "hello\nworld\n").unwrap();
        let loaded = load_content(&dir, &dir, "notes", Some(DataKind::Text)).unwrap();
        assert_eq!(loaded.kind, DataKind::Text);
        assert_eq!(loaded.data.to_value(), json!(["hello", "world"]));
    }

    #[test]
    fn loads_jsonl_as_array() {
        let dir = temp_dir();
        fs::write(
            dir.join("events.jsonl"),
            "{\"slug\":\"a\"}\n\n{\"slug\":\"b\",\"n\":2}\n",
        )
        .unwrap();
        let value = load_content(&dir, &dir, "events.jsonl", None)
            .unwrap()
            .data
            .to_value();
        assert_eq!(value, json!([{"slug": "a"}, {"slug": "b", "n": 2}]));
    }

    #[test]
    fn rejects_invalid_jsonl_line() {
        let dir = temp_dir();
        fs::write(dir.join("events.jsonl"), "{\"slug\":\"a\"}\n{bad}\n").unwrap();
        let err = load_content(&dir, &dir, "events.jsonl", None).unwrap_err();
        assert!(err.to_string().contains("line 2"));
    }

    #[test]
    fn loads_csv_as_array_of_objects() {
        let dir = temp_dir();
        fs::write(dir.join("people.csv"), "slug,name\nada,Ada\nalan,Alan\n").unwrap();
        let value = load_content(&dir, &dir, "people.csv", None)
            .unwrap()
            .data
            .to_value();
        assert_eq!(
            value,
            json!([
                {"slug": "ada", "name": "Ada"},
                {"slug": "alan", "name": "Alan"}
            ])
        );
    }

    #[test]
    fn rejects_duplicate_csv_headers() {
        let dir = temp_dir();
        fs::write(dir.join("bad.csv"), "slug,slug\na,b\n").unwrap();
        let err = load_content(&dir, &dir, "bad.csv", None).unwrap_err();
        assert!(err.to_string().contains("duplicate CSV header"));
    }
}
