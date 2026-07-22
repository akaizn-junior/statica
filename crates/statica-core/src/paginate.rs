//! Chunk a funnel array into page objects for `[page]` routes.
//!
//! # Config
//!
//! Driven by [`PaginationRule`] (from `[[pagination]]` / `--pagination SPEC`):
//! `sort_by` → `offset` → `limit` → chunk by `page_size` → optional `max_pages`.
//!
//! # Page object
//!
//! Each emitted page binds `current` to a JSON object with `items` (the chunk)
//! plus nav metadata (`page`, `pages`, `prev_href`, `next_href`, …). Authors
//! typically use `<slot … data-bind="items">` and `${prev_href}` / `${next_href}`.
//! Authors declare those fields in `<html data-bind="{…}">` (see docs/guide.md).
//!
//! This is **UI list pagination**, not sitemap URL-set splitting (see [`crate::feeds`]).

use serde_json::{json, Value};

use crate::funnel;

/// One paginated listing: chunk a data array into `[page]` folder routes.
#[derive(Debug, Clone)]
pub struct PaginationRule {
    /// Discovered page route, e.g. `blog/[page]` or `posts/[page]`.
    pub route: String,
    /// Items per generated page (≥ 1).
    pub page_size: usize,
    /// Max items from the source after `offset` (0 = unlimited).
    pub limit: usize,
    /// Skip this many items before `limit` / chunking.
    pub offset: usize,
    /// Sort by this field before slicing (empty = keep JSON order).
    pub sort_by: String,
    /// When `sort_by` is set, sort descending.
    pub sort_desc: bool,
    /// Cap how many page folders to emit (0 = unlimited).
    pub max_pages: usize,
    /// Also write page 1 at the parent path (`blog/` for `blog/[page]`).
    pub index: bool,
}

impl Default for PaginationRule {
    fn default() -> Self {
        Self {
            route: String::new(),
            page_size: 10,
            limit: 0,
            offset: 0,
            sort_by: String::new(),
            sort_desc: false,
            max_pages: 0,
            index: false,
        }
    }
}

/// One emitted pagination page (folder = `page` string).
#[derive(Debug, Clone)]
pub struct PageChunk {
    pub page: String,
    pub value: Value,
}

/// Sort → offset → limit, then return the working item list.
#[must_use]
pub fn select_items(items: &[Value], rule: &PaginationRule) -> Vec<Value> {
    let mut selected: Vec<Value> = items.to_vec();

    if !rule.sort_by.is_empty() {
        let field = rule.sort_by.as_str();
        let desc = rule.sort_desc;
        selected.sort_by(|a, b| {
            let ka = sort_key(a, field);
            let kb = sort_key(b, field);
            if desc {
                kb.cmp(&ka)
            } else {
                ka.cmp(&kb)
            }
        });
    }

    if rule.offset > 0 {
        if rule.offset >= selected.len() {
            return Vec::new();
        }
        selected = selected.split_off(rule.offset);
    }

    if rule.limit > 0 && selected.len() > rule.limit {
        selected.truncate(rule.limit);
    }

    selected
}

fn sort_key(value: &Value, field: &str) -> String {
    funnel::field_as_str(value, field).unwrap_or_default()
}

/// Split selected items into pages of `rule.page_size`.
///
/// Each page object includes `items` plus nav metadata (`page`, `pages`,
/// `prev_href` / `next_href`, `limit`, `offset`, …).
#[must_use]
pub fn chunk_items(
    items: &[Value],
    rule: &PaginationRule,
    route: &str,
    param: &str,
) -> Vec<PageChunk> {
    let selected = select_items(items, rule);
    let per = rule.page_size.max(1);
    if selected.is_empty() {
        return Vec::new();
    }

    let total_items = selected.len();
    let mut total_pages = total_items.div_ceil(per);
    if rule.max_pages > 0 {
        total_pages = total_pages.min(rule.max_pages);
    }

    // Full page-number link lists are O(pages²) in memory; skip when large.
    // Authors still get first/prev/next/last hrefs.
    const PAGES_NAV_LIMIT: usize = 200;
    let page_links = if total_pages <= PAGES_NAV_LIMIT {
        build_page_links(route, param, total_pages)
    } else {
        Vec::new()
    };
    let mut out = Vec::with_capacity(total_pages);

    for page_number in 1..=total_pages {
        let start = (page_number - 1) * per;
        let end = (start + per).min(total_items);
        let chunk = &selected[start..end];
        let page = page_number.to_string();
        let has_prev = page_number > 1;
        let has_next = page_number < total_pages;
        let prev = if has_prev {
            (page_number - 1).to_string()
        } else {
            String::new()
        };
        let next = if has_next {
            (page_number + 1).to_string()
        } else {
            String::new()
        };
        let path = route_with_param(route, param, &page);
        let href = absolute_href(&path);
        let prev_href = if has_prev {
            absolute_href(&route_with_param(route, param, &prev))
        } else {
            String::new()
        };
        let next_href = if has_next {
            absolute_href(&route_with_param(route, param, &next))
        } else {
            String::new()
        };

        let mut pages = page_links.clone();
        for link in &mut pages {
            if let Some(obj) = link.as_object_mut() {
                let is_current = obj.get("page").and_then(Value::as_str) == Some(page.as_str());
                obj.insert("current".into(), Value::Bool(is_current));
            }
        }

        out.push(PageChunk {
            page: page.clone(),
            value: json!({
                "page": page,
                "page_number": page_number,
                "total_pages": total_pages,
                "total_items": total_items,
                "source_total": items.len(),
                "per_page": per,
                "page_size": per,
                "limit": rule.limit,
                "offset": rule.offset,
                "sort_by": rule.sort_by,
                "sort_desc": rule.sort_desc,
                "max_pages": rule.max_pages,
                "has_prev": has_prev,
                "has_next": has_next,
                "prev": prev,
                "next": next,
                "path": path,
                "href": href,
                "prev_href": prev_href,
                "next_href": next_href,
                "first_href": absolute_href(&route_with_param(route, param, "1")),
                "last_href": absolute_href(&route_with_param(
                    route,
                    param,
                    &total_pages.to_string()
                )),
                "pages": pages,
                "items": chunk,
            }),
        });
    }
    out
}

/// Rewrite pagination nav paths/hrefs for a concrete locale (`[locale]` → `en`, etc.).
#[must_use]
pub fn apply_locale_to_chunk(chunk: &PageChunk, locale: &str) -> PageChunk {
    let mut value = chunk.value.clone();
    let Some(obj) = value.as_object_mut() else {
        return chunk.clone();
    };
    for key in [
        "path",
        "href",
        "prev_href",
        "next_href",
        "first_href",
        "last_href",
    ] {
        if let Some(s) = obj.get(key).and_then(Value::as_str) {
            obj.insert(key.into(), Value::String(localize_route_token(s, locale)));
        }
    }
    if let Some(pages) = obj.get_mut("pages").and_then(Value::as_array_mut) {
        for link in pages {
            if let Some(link) = link.as_object_mut() {
                for key in ["path", "href"] {
                    if let Some(s) = link.get(key).and_then(Value::as_str) {
                        link.insert(key.into(), Value::String(localize_route_token(s, locale)));
                    }
                }
            }
        }
    }
    PageChunk {
        page: chunk.page.clone(),
        value,
    }
}

fn localize_route_token(s: &str, locale: &str) -> String {
    s.replace("[locale]", locale)
}

fn build_page_links(route: &str, param: &str, total_pages: usize) -> Vec<Value> {
    (1..=total_pages)
        .map(|n| {
            let page = n.to_string();
            let path = route_with_param(route, param, &page);
            json!({
                "page": page,
                "page_number": n,
                "path": path,
                "href": absolute_href(&path),
                "current": false,
            })
        })
        .collect()
}

fn absolute_href(path: &str) -> String {
    if path.is_empty() {
        "/".into()
    } else {
        format!("/{path}/")
    }
}

fn route_with_param(route: &str, param: &str, value: &str) -> String {
    let key = format!("[{param}]");
    route
        .split('/')
        .filter(|p| !p.is_empty())
        .map(|part| if part == key { value } else { part })
        .collect::<Vec<_>>()
        .join("/")
}

/// Parent route with the `[param]` segment removed (`blog/[page]` → `blog`).
#[must_use]
pub fn index_route(route: &str, param: &str) -> String {
    let key = format!("[{param}]");
    route
        .split('/')
        .filter(|p| !p.is_empty() && *p != key.as_str())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(page_size: usize) -> PaginationRule {
        PaginationRule {
            route: "blog/[page]".into(),
            page_size,
            ..PaginationRule::default()
        }
    }

    #[test]
    fn chunks_and_meta() {
        let items: Vec<Value> = (1..=5).map(|n| json!({ "n": n })).collect();
        let pages = chunk_items(&items, &rule(2), "blog/[page]", "page");
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].page, "1");
        assert_eq!(pages[0].value["total_pages"], 3);
        assert_eq!(pages[0].value["items"].as_array().unwrap().len(), 2);
        assert_eq!(pages[0].value["path"], "blog/1");
        assert_eq!(pages[0].value["href"], "/blog/1/");
        assert_eq!(pages[0].value["next_href"], "/blog/2/");
        assert_eq!(pages[0].value["prev_href"], "");
        assert_eq!(pages[0].value["pages"].as_array().unwrap().len(), 3);
        assert_eq!(pages[0].value["pages"][0]["current"], true);
        assert_eq!(pages[2].value["items"].as_array().unwrap().len(), 1);
        assert!(!pages[2].value["has_next"].as_bool().unwrap());
    }

    #[test]
    fn limit_and_offset() {
        let items: Vec<Value> = (1..=10).map(|n| json!({ "n": n })).collect();
        let mut r = rule(3);
        r.offset = 2;
        r.limit = 5;
        let selected = select_items(&items, &r);
        assert_eq!(selected.len(), 5);
        assert_eq!(selected[0]["n"], 3);
        let pages = chunk_items(&items, &r, "blog/[page]", "page");
        assert_eq!(pages.len(), 2); // 5 items / 3
        assert_eq!(pages[0].value["total_items"], 5);
        assert_eq!(pages[0].value["source_total"], 10);
        assert_eq!(pages[0].value["offset"], 2);
        assert_eq!(pages[0].value["limit"], 5);
    }

    #[test]
    fn sort_desc_and_max_pages() {
        let items = vec![
            json!({"slug": "a", "published_at": "2026-07-01"}),
            json!({"slug": "b", "published_at": "2026-07-17"}),
            json!({"slug": "c", "published_at": "2026-07-10"}),
        ];
        let mut r = rule(1);
        r.sort_by = "published_at".into();
        r.sort_desc = true;
        r.max_pages = 2;
        let pages = chunk_items(&items, &r, "blog/[page]", "page");
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].value["items"][0]["slug"], "b");
        assert_eq!(pages[1].value["items"][0]["slug"], "c");
        assert_eq!(pages[0].value["last_href"], "/blog/2/");
    }

    #[test]
    fn index_strips_param() {
        assert_eq!(index_route("blog/[page]", "page"), "blog");
        assert_eq!(index_route("posts/page/[page]", "page"), "posts/page");
        assert_eq!(index_route("[page]", "page"), "");
    }

    #[test]
    fn apply_locale_to_chunk_rewrites_hrefs() {
        let items: Vec<Value> = (1..=4).map(|n| json!({ "n": n })).collect();
        let pages = chunk_items(&items, &rule(2), "[locale]/blog/[page]", "page");
        let localized = apply_locale_to_chunk(&pages[0], "en");
        assert_eq!(localized.value["path"], "en/blog/1");
        assert_eq!(localized.value["href"], "/en/blog/1/");
        assert_eq!(localized.value["next_href"], "/en/blog/2/");
    }
}
