//! Sitemap + RSS feeds written into `out_dir` after pages emit.
//!
//! - **Sitemap** — [`sitemap_rs`]: single `<urlset>`, or paginated part files
//!   rolled up into a `<sitemapindex>` when URL count exceeds `urls_per_file`.
//! - **RSS** — [`rss`] crate builders; items from collection pages / funnel arrays.
//!
//! Both require a non-empty `site_url` (absolute origin). Missing origin → warning
//! and skip (does not fail the build).

use std::fs;
use std::path::{Path, PathBuf};


use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use rss::{ChannelBuilder, GuidBuilder, Item, ItemBuilder};
use serde_json::Value;
use sitemap_rs::sitemap::Sitemap;
use sitemap_rs::sitemap_index::SitemapIndex;
use sitemap_rs::url::Url as SitemapUrl;
use sitemap_rs::url_set::UrlSet;

use crate::bind;
use crate::discover::{PageKind, PageSource};
use crate::error::{Error, Result};
use crate::funnel::{self, DataSource};
use crate::loc::Diagnostic;

/// Protocol max URLs (or nested sitemaps) per file.
pub const SITEMAP_URL_LIMIT: usize = 50_000;

#[derive(Debug, Clone)]
pub struct SitemapOptions {
    pub enabled: bool,
    pub filename: String,
    /// URLs per sitemap file before splitting into parts + a sitemap index.
    /// Clamped to `1..=50_000` (protocol limit).
    pub urls_per_file: usize,
}

impl Default for SitemapOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            filename: "sitemap.xml".into(),
            urls_per_file: SITEMAP_URL_LIMIT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RssOptions {
    pub enabled: bool,
    pub filename: String,
    pub title: String,
    pub description: String,
    pub language: String,
    /// Max items (0 = unlimited).
    pub limit: usize,
    pub title_field: String,
    pub description_field: String,
    pub date_field: String,
    /// Data source ids to include; empty = every collection page.
    pub collections: Vec<String>,
}

impl Default for RssOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            filename: "rss.xml".into(),
            title: String::new(),
            description: String::new(),
            language: "en".into(),
            limit: 50,
            title_field: "headline".into(),
            description_field: "summary".into(),
            date_field: "published_at".into(),
            collections: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FeedPage<'a> {
    pub source: &'a PageSource,
    pub data: &'a std::collections::HashMap<String, DataSource>,
    pub collection_id: Option<String>,
}

/// Write sitemap.xml / rss.xml when enabled. Returns warnings (e.g. missing site_url).
pub fn write_feeds(
    out_dir: &Path,
    site_url: &str,
    sitemap: &SitemapOptions,
    rss: &RssOptions,
    page_outputs: &[PathBuf],
    feed_pages: &[FeedPage<'_>],
) -> Result<Vec<Diagnostic>> {
    let mut warnings = Vec::new();
    let base = site_url.trim().trim_end_matches('/');

    if sitemap.enabled {
        if base.is_empty() {
            warnings.push(Diagnostic::at_file(
                "statica.toml",
                "sitemap enabled but site_url is empty — skipped (set site_url in statica.toml)",
            ));
        } else {
            write_sitemap(out_dir, base, sitemap, page_outputs)?;
        }
    }

    if rss.enabled {
        if base.is_empty() {
            warnings.push(Diagnostic::at_file(
                "statica.toml",
                "rss enabled but site_url is empty — skipped (set site_url in statica.toml)",
            ));
        } else {
            let mut items = collect_rss_items(base, rss, feed_pages);
            // Newest first when date looks sortable (ISO / YYYY-MM-DD).
            items.sort_by(|a, b| b.date.cmp(&a.date));
            if rss.limit > 0 && items.len() > rss.limit {
                items.truncate(rss.limit);
            }
            let title = if rss.title.is_empty() {
                "Feed"
            } else {
                rss.title.as_str()
            };
            let channel = ChannelBuilder::default()
                .title(title)
                .link(format!("{base}/"))
                .description(rss.description.as_str())
                .language(Some(rss.language.clone()))
                .items(items.into_iter().map(into_rss_item).collect::<Vec<_>>())
                .build();
            let path = out_dir.join(&rss.filename);
            fs::write(&path, channel.to_string())?;
        }
    }

    Ok(warnings)
}

fn write_sitemap(
    out_dir: &Path,
    base: &str,
    opts: &SitemapOptions,
    page_outputs: &[PathBuf],
) -> Result<()> {
    let urls: Vec<SitemapUrl> = page_outputs
        .iter()
        .filter_map(|p| url_from_output(out_dir, p))
        .filter_map(|path| {
            SitemapUrl::builder(format!("{base}{path}"))
                .build()
                .ok()
        })
        .collect();

    let per = opts.urls_per_file.clamp(1, SITEMAP_URL_LIMIT);

    if urls.len() <= per {
        let xml = render_urlset(urls)?;
        fs::write(out_dir.join(&opts.filename), xml)?;
        return Ok(());
    }

    // Paginate into part files, then roll up into a sitemap index at `filename`.
    let mut index_entries = Vec::with_capacity(urls.len().div_ceil(per));
    for (i, chunk) in urls.chunks(per).enumerate() {
        let part_name = part_filename(&opts.filename, i + 1);
        let part_path = out_dir.join(&part_name);
        if let Some(parent) = part_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&part_path, render_urlset(chunk.to_vec())?)?;
        index_entries.push(Sitemap::new(format!("{base}/{part_name}"), None));
    }

    let index_path = out_dir.join(&opts.filename);
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&index_path, render_sitemap_index(index_entries)?)?;
    Ok(())
}

fn render_urlset(urls: Vec<SitemapUrl>) -> Result<Vec<u8>> {
    let set = UrlSet::new(urls).map_err(|e| Error::at_file("sitemap.xml", e.to_string()))?;
    let mut buf = Vec::new();
    set.write(&mut buf)
        .map_err(|e| Error::at_file("sitemap.xml", format!("sitemap write: {e}")))?;
    Ok(buf)
}

fn render_sitemap_index(sitemaps: Vec<Sitemap>) -> Result<Vec<u8>> {
    let index = SitemapIndex::new(sitemaps).map_err(|e| Error::at_file("sitemap.xml", e.to_string()))?;
    let mut buf = Vec::new();
    index
        .write(&mut buf)
        .map_err(|e| Error::at_file("sitemap.xml", format!("sitemap index write: {e}")))?;
    Ok(buf)
}

/// `sitemap.xml` + page 3 → `sitemap-3.xml`; keeps parent dirs.
fn part_filename(index_name: &str, n: usize) -> String {
    let path = Path::new(index_name);
    let file = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("sitemap.xml");
    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sitemap");
    let ext = Path::new(file)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("xml");
    let part = format!("{stem}-{n}.{ext}");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            parent.join(part).to_string_lossy().replace('\\', "/")
        }
        _ => part,
    }
}

fn url_from_output(out_dir: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(out_dir).ok()?;
    let parts: Vec<String> = rel
        .components()
        .filter_map(|c| {
            let s = c.as_os_str().to_string_lossy();
            if s == "index.html" {
                None
            } else {
                Some(s.into_owned())
            }
        })
        .collect();
    if parts.is_empty() {
        Some("/".into())
    } else {
        Some(format!("/{}/", parts.join("/")))
    }
}

fn collection_id_from_doc(doc: &crate::parse::Document) -> Option<String> {
    bind::html_collection_id(doc)
}

pub fn collection_id_for_page(doc: &crate::parse::Document) -> Option<String> {
    collection_id_from_doc(doc)
}

struct RssItem {
    title: String,
    link: String,
    description: String,
    date: String,
}

fn collect_rss_items(
    base: &str,
    rss: &RssOptions,
    feed_pages: &[FeedPage<'_>],
) -> Vec<RssItem> {
    let mut items = Vec::new();
    for page in feed_pages {
        if page.source.kind() != PageKind::Collection {
            continue;
        }
        let Some(id) = page.collection_id.as_deref() else {
            continue;
        };
        if !rss.collections.is_empty() && !rss.collections.iter().any(|c| c == id) {
            continue;
        }
        let Some(list) = page.data.get(id) else {
            continue;
        };
        let Value::Array(arr) = &list.value else {
            continue;
        };
        let Some(param) = page.source.params.first() else {
            continue;
        };
        for entry in arr {
            let Some(folder) = funnel::field_as_str(entry, param) else {
                continue;
            };
            let path = route_to_url(&page.source.route, param, &folder);
            let title = funnel::field_as_str(entry, &rss.title_field)
                .unwrap_or_else(|| folder.clone());
            let description =
                funnel::field_as_str(entry, &rss.description_field).unwrap_or_default();
            let date = funnel::field_as_str(entry, &rss.date_field).unwrap_or_default();
            items.push(RssItem {
                title,
                link: format!("{base}{path}"),
                description,
                date,
            });
        }
    }
    items
}

fn into_rss_item(item: RssItem) -> Item {
    let guid = GuidBuilder::default()
        .value(item.link.clone())
        .permalink(true)
        .build();
    let mut builder = ItemBuilder::default();
    builder
        .title(Some(item.title))
        .link(Some(item.link))
        .description(Some(item.description))
        .guid(Some(guid));
    if let Some(pub_date) = to_rfc2822(&item.date) {
        builder.pub_date(Some(pub_date));
    }
    builder.build()
}

fn route_to_url(route: &str, param: &str, value: &str) -> String {
    let key = format!("[{param}]");
    let mut parts = Vec::new();
    for part in route.split('/') {
        if part.is_empty() {
            continue;
        }
        if part == key {
            parts.push(value);
        } else {
            parts.push(part);
        }
    }
    if parts.is_empty() {
        "/".into()
    } else {
        format!("/{}/", parts.join("/"))
    }
}

/// Best-effort RFC 2822 for RSS `pubDate` via chrono.
fn to_rfc2822(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc2822(t) {
        return Some(dt.to_rfc2822());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Some(dt.with_timezone(&Utc).to_rfc2822());
    }
    if t.len() >= 10 {
        if let Ok(d) = NaiveDate::parse_from_str(&t[..10], "%Y-%m-%d") {
            if let Some(naive) = d.and_hms_opt(0, 0, 0) {
                return Some(Utc.from_utc_datetime(&naive).to_rfc2822());
            }
        }
    }
    Some(t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("statica-feeds-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn part_names() {
        assert_eq!(part_filename("sitemap.xml", 1), "sitemap-1.xml");
        assert_eq!(part_filename("feeds/map.xml", 2), "feeds/map-2.xml");
    }

    #[test]
    fn single_sitemap_when_under_limit() {
        let dir = temp_dir();
        let pages: Vec<PathBuf> = ["index.html", "about/index.html"]
            .into_iter()
            .map(|p| {
                let path = dir.join(p);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::write(&path, "").unwrap();
                path
            })
            .collect();
        write_sitemap(
            &dir,
            "https://ex.com",
            &SitemapOptions {
                enabled: true,
                filename: "sitemap.xml".into(),
                urls_per_file: 50_000,
            },
            &pages,
        )
        .unwrap();
        let xml = fs::read_to_string(dir.join("sitemap.xml")).unwrap();
        assert!(xml.contains("<urlset"));
        assert!(xml.contains("<loc>https://ex.com/</loc>"));
        assert!(!dir.join("sitemap-1.xml").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn paginates_into_index() {
        let dir = temp_dir();
        let pages: Vec<PathBuf> = (0..5)
            .map(|i| {
                let path = dir.join(format!("p{i}/index.html"));
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, "").unwrap();
                path
            })
            .collect();
        write_sitemap(
            &dir,
            "https://ex.com",
            &SitemapOptions {
                enabled: true,
                filename: "sitemap.xml".into(),
                urls_per_file: 2,
            },
            &pages,
        )
        .unwrap();
        let index = fs::read_to_string(dir.join("sitemap.xml")).unwrap();
        assert!(index.contains("<sitemapindex"));
        assert!(index.contains("<loc>https://ex.com/sitemap-1.xml</loc>"));
        assert!(index.contains("<loc>https://ex.com/sitemap-2.xml</loc>"));
        assert!(index.contains("<loc>https://ex.com/sitemap-3.xml</loc>"));
        let part1 = fs::read_to_string(dir.join("sitemap-1.xml")).unwrap();
        assert!(part1.contains("<urlset"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn route_replaces_slug() {
        assert_eq!(
            route_to_url("posts/[slug]", "slug", "hello"),
            "/posts/hello/"
        );
    }

    #[test]
    fn rfc_date() {
        assert_eq!(
            to_rfc2822("2026-07-01").as_deref(),
            Some("Wed, 1 Jul 2026 00:00:00 +0000")
        );
    }

    #[test]
    fn rss_channel_builds() {
        let channel = ChannelBuilder::default()
            .title("Blog")
            .link("https://ex.com/")
            .description("posts")
            .language(Some("en".into()))
            .items(vec![into_rss_item(RssItem {
                title: "Hello".into(),
                link: "https://ex.com/posts/hello/".into(),
                description: "hi".into(),
                date: "2026-07-01".into(),
            })])
            .build();
        let xml = channel.to_string();
        assert!(xml.contains("<title>Hello</title>"));
        assert!(xml.contains("<pubDate>Wed, 1 Jul 2026 00:00:00 +0000</pubDate>"));
    }
}
