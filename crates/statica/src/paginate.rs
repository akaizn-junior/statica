//! Chunk a funnel array into page objects for `[page]` routes.
//!
//! # Config
//!
//! Driven by [`PaginationRule`] (from `[[pagination]]` / `--pagination SPEC`):
//! `sort_by` → `offset` → `limit` → chunk by `page_size` → optional `max_pages`.
//!
//! # Page object
//!
//! Each emitted page stores `items` (the chunk) plus nav metadata (`page`,
//! `pages`, `prev_href`, `next_href`, …). Page templates read that object at
//! `page.pagination`; fragments mounted from the page receive the pagination
//! chunk as their current value.
//!
//! This is **UI list pagination**, not sitemap URL-set splitting (see [`crate::feeds`]).

use serde_json::{Map, Value};

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

/// Fields exposed in `page.pagination` for paginated routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaginationField {
    /// Page folder value as a string, e.g. `"3"`.
    Page,
    /// Page number as an integer.
    PageNumber,
    /// Total generated pagination pages.
    TotalPages,
    /// Number of items after offset/limit.
    TotalItems,
    /// Number of source items before offset/limit.
    SourceTotal,
    /// Items per page.
    PerPage,
    /// Alias for `per_page`.
    PageSize,
    /// Configured item limit.
    Limit,
    /// Configured item offset.
    Offset,
    /// Field used for sorting.
    SortBy,
    /// Whether sorting is descending.
    SortDesc,
    /// Configured max pages.
    MaxPages,
    /// Whether a previous page exists.
    HasPrev,
    /// Whether a next page exists.
    HasNext,
    /// Previous page folder value.
    Prev,
    /// Next page folder value.
    Next,
    /// Route path for this page without leading/trailing slash.
    Path,
    /// Absolute href for this page.
    Href,
    /// Absolute href for the previous page.
    PrevHref,
    /// Absolute href for the next page.
    NextHref,
    /// Absolute href for the first page.
    FirstHref,
    /// Absolute href for the last page.
    LastHref,
    /// Optional full page-number navigation list.
    Pages,
    /// Whether a page-number navigation entry is the current page.
    Current,
    /// Current chunk items.
    Items,
}

impl PaginationField {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::PageNumber => "page_number",
            Self::TotalPages => "total_pages",
            Self::TotalItems => "total_items",
            Self::SourceTotal => "source_total",
            Self::PerPage => "per_page",
            Self::PageSize => "page_size",
            Self::Limit => "limit",
            Self::Offset => "offset",
            Self::SortBy => "sort_by",
            Self::SortDesc => "sort_desc",
            Self::MaxPages => "max_pages",
            Self::HasPrev => "has_prev",
            Self::HasNext => "has_next",
            Self::Prev => "prev",
            Self::Next => "next",
            Self::Path => "path",
            Self::Href => "href",
            Self::PrevHref => "prev_href",
            Self::NextHref => "next_href",
            Self::FirstHref => "first_href",
            Self::LastHref => "last_href",
            Self::Pages => "pages",
            Self::Current => "current",
            Self::Items => "items",
        }
    }
}

/// Returns true when an object has the required shape of a pagination chunk.
#[must_use]
pub fn is_pagination_chunk(obj: &Map<String, Value>) -> bool {
    obj.contains_key(PaginationField::Items.as_str())
        && obj.contains_key(PaginationField::TotalPages.as_str())
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
                let is_current = obj
                    .get(PaginationField::Page.as_str())
                    .and_then(Value::as_str)
                    == Some(page.as_str());
                obj.insert(
                    PaginationField::Current.as_str().into(),
                    Value::Bool(is_current),
                );
            }
        }

        let value = pagination_value(PaginationValueInput {
            page: page.clone(),
            page_number,
            total_pages,
            total_items,
            source_total: items.len(),
            per,
            rule,
            has_prev,
            has_next,
            prev,
            next,
            path,
            href,
            prev_href,
            next_href,
            first_href: absolute_href(&route_with_param(route, param, "1")),
            last_href: absolute_href(&route_with_param(route, param, &total_pages.to_string())),
            pages,
            chunk,
        });

        out.push(PageChunk {
            page: page.clone(),
            value,
        });
    }
    out
}

struct PaginationValueInput<'a> {
    page: String,
    page_number: usize,
    total_pages: usize,
    total_items: usize,
    source_total: usize,
    per: usize,
    rule: &'a PaginationRule,
    has_prev: bool,
    has_next: bool,
    prev: String,
    next: String,
    path: String,
    href: String,
    prev_href: String,
    next_href: String,
    first_href: String,
    last_href: String,
    pages: Vec<Value>,
    chunk: &'a [Value],
}

fn pagination_value(input: PaginationValueInput<'_>) -> Value {
    let mut obj = Map::new();
    insert(&mut obj, PaginationField::Page, input.page);
    insert(&mut obj, PaginationField::PageNumber, input.page_number);
    insert(&mut obj, PaginationField::TotalPages, input.total_pages);
    insert(&mut obj, PaginationField::TotalItems, input.total_items);
    insert(&mut obj, PaginationField::SourceTotal, input.source_total);
    insert(&mut obj, PaginationField::PerPage, input.per);
    insert(&mut obj, PaginationField::PageSize, input.per);
    insert(&mut obj, PaginationField::Limit, input.rule.limit);
    insert(&mut obj, PaginationField::Offset, input.rule.offset);
    insert(
        &mut obj,
        PaginationField::SortBy,
        input.rule.sort_by.clone(),
    );
    insert(&mut obj, PaginationField::SortDesc, input.rule.sort_desc);
    insert(&mut obj, PaginationField::MaxPages, input.rule.max_pages);
    insert(&mut obj, PaginationField::HasPrev, input.has_prev);
    insert(&mut obj, PaginationField::HasNext, input.has_next);
    insert(&mut obj, PaginationField::Prev, input.prev);
    insert(&mut obj, PaginationField::Next, input.next);
    insert(&mut obj, PaginationField::Path, input.path);
    insert(&mut obj, PaginationField::Href, input.href);
    insert(&mut obj, PaginationField::PrevHref, input.prev_href);
    insert(&mut obj, PaginationField::NextHref, input.next_href);
    insert(&mut obj, PaginationField::FirstHref, input.first_href);
    insert(&mut obj, PaginationField::LastHref, input.last_href);
    insert(&mut obj, PaginationField::Pages, input.pages);
    insert(&mut obj, PaginationField::Items, input.chunk);
    Value::Object(obj)
}

fn insert(field_map: &mut Map<String, Value>, field: PaginationField, value: impl Into<Value>) {
    field_map.insert(field.as_str().into(), value.into());
}

/// Rewrite pagination nav paths/hrefs for a concrete locale (`[locale]` → `en`, etc.).
#[must_use]
pub fn apply_locale_to_chunk(chunk: &PageChunk, locale: &str) -> PageChunk {
    let mut value = chunk.value.clone();
    let Some(obj) = value.as_object_mut() else {
        return chunk.clone();
    };
    for key in [
        PaginationField::Path,
        PaginationField::Href,
        PaginationField::PrevHref,
        PaginationField::NextHref,
        PaginationField::FirstHref,
        PaginationField::LastHref,
    ] {
        if let Some(s) = obj.get(key.as_str()).and_then(Value::as_str) {
            obj.insert(
                key.as_str().into(),
                Value::String(localize_route_token(s, locale)),
            );
        }
    }
    if let Some(pages) = obj
        .get_mut(PaginationField::Pages.as_str())
        .and_then(Value::as_array_mut)
    {
        for link in pages {
            if let Some(link) = link.as_object_mut() {
                for key in [PaginationField::Path, PaginationField::Href] {
                    if let Some(s) = link.get(key.as_str()).and_then(Value::as_str) {
                        link.insert(
                            key.as_str().into(),
                            Value::String(localize_route_token(s, locale)),
                        );
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
            let href = absolute_href(&path);
            let mut link = Map::new();
            insert(&mut link, PaginationField::Page, page);
            insert(&mut link, PaginationField::PageNumber, n);
            insert(&mut link, PaginationField::Path, path);
            insert(&mut link, PaginationField::Href, href);
            insert(&mut link, PaginationField::Current, false);
            Value::Object(link)
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
