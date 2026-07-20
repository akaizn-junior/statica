//! Build orchestration: discover → funnel → bind → scope → emit.
//!
//! [`build`] is the main entry. Collection pages expand 1:N via
//! [`emit_collection`]; routes listed in [`BuildOptions::pagination`] expand via
//! [`emit_paginated`] (see [`crate::paginate`]).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rayon::prelude::*;
use serde_json::Value;

use crate::assets::AssetProcessOptions;
use crate::bind;
use crate::discover::{self, PageKind, PageSource};
use crate::emit;
use crate::error::{Error, Result};
use crate::feeds::{self, FeedPage, RssOptions, SitemapOptions};
use crate::fragment::FragmentRegistry;
use crate::funnel::{self, DataSource};
use crate::paginate::{self, PaginationRule};
use crate::parse::{self, Document};
use crate::EmitOptions;

/// Inputs for a build. The CLI maps `statica.toml` into this; core does not read config files.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub root: PathBuf,
    pub out_dir: PathBuf,
    pub copy_assets: bool,
    /// Absolute site origin for sitemap/RSS (e.g. `https://example.com`). Empty → feeds skipped.
    pub site_url: String,
    pub sitemap: SitemapOptions,
    pub rss: RssOptions,
    /// List → `…/1/`, `…/2/`, … expansions.
    pub pagination: Vec<PaginationRule>,
    /// Asset optimize pipeline (off unless `enabled`; kinds are selectable).
    pub process: AssetProcessOptions,
    /// What to strip / tidy when writing HTML.
    pub emit: EmitOptions,
    pub clean: bool,
    pub asset_dirs: Vec<String>,
    pub ignore_dirs: Vec<String>,
}

impl BuildOptions {
    /// Pipeline defaults (no config file). Prefer the CLI for end-user settings.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            out_dir: root.join(".dist"),
            copy_assets: true,
            site_url: String::new(),
            sitemap: SitemapOptions::default(),
            rss: RssOptions::default(),
            pagination: Vec::new(),
            process: AssetProcessOptions::default(),
            emit: EmitOptions::default(),
            clean: true,
            asset_dirs: vec!["public".into(), "assets".into(), "static".into()],
            ignore_dirs: vec![
                ".dist".into(),
                "dist".into(),
                "target".into(),
                ".git".into(),
            ],
            root,
        }
    }

    fn pagination_for(&self, route: &str) -> Option<&PaginationRule> {
        self.pagination.iter().find(|r| r.route == route)
    }
}

#[derive(Debug, Default)]
pub struct BuildReport {
    pub pages_written: usize,
    pub assets_processed: usize,
    pub warnings: Vec<String>,
    pub duration_ms: u128,
    pub outputs: Vec<PathBuf>,
}

struct PreparedPage {
    source: PageSource,
    doc: Document,
    data: std::collections::HashMap<String, DataSource>,
}

pub fn build(opts: &BuildOptions) -> Result<BuildReport> {
    let started = Instant::now();

    if opts.clean && opts.out_dir.exists() {
        fs::remove_dir_all(&opts.out_dir)?;
    }
    fs::create_dir_all(&opts.out_dir)?;

    let pages = discover::discover_pages(&opts.root, &opts.ignore_dirs)?;
    let (registry, prepared) = prepare_pages(&pages)?;
    let registry = Arc::new(registry);

    let warnings = Mutex::new(Vec::new());
    let results: Vec<Result<Vec<PathBuf>>> = prepared
        .par_iter()
        .map(|page| emit_prepared(opts, page, &registry, &warnings))
        .collect();

    let mut outputs = Vec::new();
    for chunk in results {
        outputs.extend(chunk?);
    }

    let mut warnings = warnings
        .into_inner()
        .map_err(|_| Error::msg("warnings mutex poisoned"))?;

    let mut assets_processed = 0;
    if opts.copy_assets {
        let assets = emit::copy_static_assets(
            &opts.root,
            &opts.out_dir,
            &opts.asset_dirs,
            &opts.process,
        )?;
        assets_processed = assets.processed;
        warnings.extend(assets.warnings);
    }

    let feed_pages: Vec<FeedPage<'_>> = prepared
        .iter()
        .map(|p| FeedPage {
            source: &p.source,
            data: &p.data,
            collection_id: feeds::collection_id_for_page(&p.doc),
        })
        .collect();
    warnings.extend(feeds::write_feeds(
        &opts.out_dir,
        &opts.site_url,
        &opts.sitemap,
        &opts.rss,
        &outputs,
        &feed_pages,
    )?);

    Ok(BuildReport {
        pages_written: outputs.len(),
        assets_processed,
        warnings,
        duration_ms: started.elapsed().as_millis(),
        outputs,
    })
}

fn prepare_pages(pages: &[PageSource]) -> Result<(FragmentRegistry, Vec<PreparedPage>)> {
    let mut registry = FragmentRegistry::new();
    let mut prepared = Vec::with_capacity(pages.len());

    for page in pages {
        let html = fs::read_to_string(&page.path)
            .map_err(|e| Error::page(page.path.display().to_string(), e.to_string()))?;
        let doc = parse::parse_document(&html)
            .map_err(|e| Error::page(page.path.display().to_string(), e.to_string()))?;
        let dir = page.path.parent().unwrap_or_else(|| Path::new("."));
        let data = funnel::load_data_from_document(&doc, dir, registry.data_cache_mut())?;
        registry.load_links_from_document(&doc, dir)?;
        prepared.push(PreparedPage {
            source: page.clone(),
            doc,
            data,
        });
    }
    Ok((registry, prepared))
}

fn emit_prepared(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    warnings: &Mutex<Vec<String>>,
) -> Result<Vec<PathBuf>> {
    if let Some(rule) = opts.pagination_for(&page.source.route) {
        return emit_paginated(opts, page, registry, rule, warnings);
    }
    match page.source.kind() {
        PageKind::Static => {
            let rendered =
                bind::render_page_document(registry, &page.doc, None, &page.data, &opts.emit)
                    .map_err(|e| {
                        Error::page(page.source.path.display().to_string(), e.to_string())
                    })?;
            let out = emit::out_path_for_route(&opts.out_dir, &page.source.route, None);
            emit::write_html(&out, &rendered)?;
            Ok(vec![out])
        }
        PageKind::Collection => emit_collection(opts, page, registry, warnings),
    }
}

fn html_data_bind(doc: &Document) -> Option<String> {
    doc.children.iter().find_map(|n| match n {
        crate::parse::Node::Element(el) if el.name.eq_ignore_ascii_case("html") => {
            el.attr("data-bind").map(str::to_string)
        }
        _ => None,
    })
}

fn emit_paginated(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    rule: &PaginationRule,
    warnings: &Mutex<Vec<String>>,
) -> Result<Vec<PathBuf>> {
    let collection_id = html_data_bind(&page.doc).ok_or_else(|| {
        Error::page(
            page.source.path.display().to_string(),
            "paginated page needs data-bind on <html> pointing at a statica/data id",
        )
    })?;

    let param = page
        .source
        .params
        .first()
        .ok_or_else(|| {
            Error::page(
                page.source.path.display().to_string(),
                format!(
                    "pagination route `{}` needs a [param] segment (e.g. blog/[page])",
                    page.source.route
                ),
            )
        })?
        .clone();

    if page.source.params.len() > 1 {
        return Err(Error::page(
            page.source.path.display().to_string(),
            "pagination routes support a single [param] (the page number folder)",
        ));
    }

    let list = page
        .data
        .get(&collection_id)
        .ok_or_else(|| Error::MissingData {
            id: collection_id.clone(),
        })?;

    let items = match &list.value {
        Value::Array(a) => a.as_slice(),
        other => {
            return Err(Error::page(
                page.source.path.display().to_string(),
                format!("pagination `{collection_id}` must be an array, got {other}"),
            ));
        }
    };

    let chunks = paginate::chunk_items(items, rule, &page.source.route, &param);
    if chunks.is_empty() {
        let mut w = warnings
            .lock()
            .map_err(|_| Error::msg("warnings mutex poisoned"))?;
        w.push(format!(
            "{}: pagination `{collection_id}` is empty — 0 pages emitted",
            page.source.route
        ));
        return Ok(Vec::new());
    }

    let mut outs = Vec::with_capacity(chunks.len() + usize::from(rule.index));
    for chunk in &chunks {
        let rendered = bind::render_page_document(
            registry,
            &page.doc,
            Some(&chunk.value),
            &page.data,
            &opts.emit,
        )
        .map_err(|e| Error::page(page.source.path.display().to_string(), e.to_string()))?;
        let out = emit::out_path_for_route(
            &opts.out_dir,
            &page.source.route,
            Some((&param, &chunk.page)),
        );
        emit::write_html(&out, &rendered)?;
        outs.push(out);
    }

    if rule.index {
        if let Some(first) = chunks.first() {
            let rendered = bind::render_page_document(
                registry,
                &page.doc,
                Some(&first.value),
                &page.data,
                &opts.emit,
            )
            .map_err(|e| Error::page(page.source.path.display().to_string(), e.to_string()))?;
            let index_route = paginate::index_route(&page.source.route, &param);
            let out = emit::out_path_for_route(&opts.out_dir, &index_route, None);
            emit::write_html(&out, &rendered)?;
            outs.push(out);
        }
    }

    Ok(outs)
}

fn emit_collection(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    warnings: &Mutex<Vec<String>>,
) -> Result<Vec<PathBuf>> {
    let collection_id = html_data_bind(&page.doc).ok_or_else(|| {
        Error::page(
            page.source.path.display().to_string(),
            "collection page needs data-bind on <html> pointing at a statica/data id",
        )
    })?;

    let list = page
        .data
        .get(&collection_id)
        .ok_or_else(|| Error::MissingData {
            id: collection_id.clone(),
        })?;

    let items = match &list.value {
        Value::Array(a) => a,
        other => {
            return Err(Error::page(
                page.source.path.display().to_string(),
                format!("collection `{collection_id}` must be an array, got {other}"),
            ));
        }
    };

    if items.is_empty() {
        let mut w = warnings
            .lock()
            .map_err(|_| Error::msg("warnings mutex poisoned"))?;
        w.push(format!(
            "{}: collection `{collection_id}` is empty — 0 pages emitted",
            page.source.route
        ));
        return Ok(Vec::new());
    }

    let param = page
        .source
        .params
        .first()
        .ok_or_else(|| Error::msg("collection without params"))?;

    let mut seen = HashSet::with_capacity(items.len());
    let mut outs = Vec::with_capacity(items.len());

    for item in items {
        let folder = funnel::field_as_str(item, param).ok_or_else(|| Error::MissingRouteField {
            field: param.clone(),
        })?;
        if !seen.insert(folder.clone()) {
            return Err(Error::DuplicateRouteValue {
                field: param.clone(),
                value: folder,
            });
        }
        let rendered =
            bind::render_page_document(registry, &page.doc, Some(item), &page.data, &opts.emit)
                .map_err(|e| Error::page(page.source.path.display().to_string(), e.to_string()))?;
        let out =
            emit::out_path_for_route(&opts.out_dir, &page.source.route, Some((param, &folder)));
        emit::write_html(&out, &rendered)?;
        outs.push(out);
    }
    Ok(outs)
}

pub fn rebuild_paths(opts: &BuildOptions, changed: &[PathBuf]) -> Result<BuildReport> {
    let meaningful: Vec<&PathBuf> = changed
        .iter()
        .filter(|p| {
            if p.starts_with(&opts.out_dir) {
                return false;
            }
            let s = p.to_string_lossy();
            !s.contains("/target/")
        })
        .collect();

    if meaningful.is_empty() && !changed.is_empty() {
        return Ok(BuildReport::default());
    }

    let mut incremental = opts.clone();
    incremental.clean = meaningful.is_empty();
    build(&incremental)
}
