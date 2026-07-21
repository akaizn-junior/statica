//! Build orchestration: discover → pre/parse → funnel → bind → scope → emit.
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

use crate::aliases::AliasOptions;
use crate::assets::AssetProcessOptions;
use crate::bind;
use crate::build_log::BuildLog;
use crate::discover::{self, PageKind, PageSource};
use crate::emit;
use crate::error::{Error, Result};
use crate::feeds::{self, FeedPage, RssOptions, SitemapOptions};
use crate::fragment::FragmentRegistry;
use crate::funnel::{self, DataSource};
use crate::loc::Diagnostic;
use crate::paginate::{self, PaginationRule};
use crate::parse::{self, Document};
use crate::{EmitOptions, FormsOptions};

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
    /// Path / URL aliases for authoring (`[aliases]` in statica.toml).
    pub aliases: AliasOptions,
    /// Static form wiring (`[forms]` in statica.toml).
    pub forms: FormsOptions,
    pub clean: bool,
    pub asset_dirs: Vec<String>,
    pub ignore_dirs: Vec<String>,
    /// Emit step lines to stderr during the build (CLI: `--verbose`).
    pub verbose: bool,
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
            aliases: AliasOptions::default(),
            forms: FormsOptions::default(),
            clean: true,
            asset_dirs: vec!["public".into(), "assets".into(), "static".into()],
            ignore_dirs: vec![
                ".dist".into(),
                "dist".into(),
                "target".into(),
                ".git".into(),
            ],
            root,
            verbose: false,
        }
    }

    fn pagination_for(&self, route: &str) -> Option<&PaginationRule> {
        self.pagination.iter().find(|r| r.route == route)
    }

    fn log(&self) -> BuildLog {
        BuildLog::new(self.verbose)
    }
}

/// One timed pipeline step (for `--verbose` summary).
#[derive(Debug, Clone)]
pub struct BuildPhase {
    pub name: &'static str,
    pub duration_ms: u128,
    pub detail: String,
}

/// Pages emitted for one discovered source route.
#[derive(Debug, Clone)]
pub struct BuildRouteRow {
    pub route: String,
    pub kind: PageKind,
    /// True when expanded via `[[pagination]]` (still a `[param]` source route).
    pub paginated: bool,
    pub pages: usize,
}

#[derive(Debug, Default)]
pub struct BuildReport {
    pub pages_written: usize,
    pub assets_processed: usize,
    pub warnings: Vec<Diagnostic>,
    pub duration_ms: u128,
    pub outputs: Vec<PathBuf>,
    pub phases: Vec<BuildPhase>,
    pub routes: Vec<BuildRouteRow>,
    pub sources: usize,
    pub fragments: usize,
    pub data_sources: usize,
}

struct PreparedPage {
    source: PageSource,
    html: String,
    doc: Document,
    data: std::collections::HashMap<String, DataSource>,
}

struct EmitResult {
    outputs: Vec<PathBuf>,
    route: BuildRouteRow,
}

impl PreparedPage {
    fn file(&self) -> String {
        self.source.path.display().to_string()
    }

    fn render(
        &self,
        registry: &FragmentRegistry,
        current: Option<&Value>,
        emit: &EmitOptions,
        aliases: &AliasOptions,
        forms: &FormsOptions,
    ) -> Result<String> {
        let file = self.file();
        bind::render_page_document(
            registry,
            &self.doc,
            current,
            &self.data,
            emit,
            aliases,
            forms,
            Some((file.as_str(), self.html.as_str())),
        )
        .map_err(|e| e.in_file(&file, &self.html))
    }

    fn at(&self, needles: &[&str], message: impl Into<String>) -> Error {
        Error::at(&self.file(), &self.html, needles, message)
    }

    fn warn(&self, needles: &[&str], message: impl Into<String>) -> Diagnostic {
        Diagnostic::at(&self.file(), &self.html, needles, message)
    }

    fn route_row(&self, pages: usize, kind: PageKind, paginated: bool) -> BuildRouteRow {
        BuildRouteRow {
            route: self.source.route.clone(),
            kind,
            paginated,
            pages,
        }
    }
}

pub fn build(opts: &BuildOptions) -> Result<BuildReport> {
    let started = Instant::now();
    let log = opts.log();
    let mut phases = Vec::new();

    if opts.clean && opts.out_dir.exists() {
        log.step("clean  output directory");
        fs::remove_dir_all(&opts.out_dir)?;
    }
    fs::create_dir_all(&opts.out_dir)?;

    let t = Instant::now();
    let pages = discover::discover_pages(&opts.root, &opts.ignore_dirs)?;
    let discover_ms = t.elapsed().as_millis();
    let sources = pages.len();
    phases.push(BuildPhase {
        name: "discover",
        duration_ms: discover_ms,
        detail: format!("{sources} sources"),
    });
    log.step(&format!("discover  {sources} sources ({discover_ms}ms)"));

    let t = Instant::now();
    let (registry, prepared, data_sources) = prepare_pages(&pages, &opts.aliases)?;
    let prepare_ms = t.elapsed().as_millis();
    let fragments = registry.len();
    phases.push(BuildPhase {
        name: "funnel",
        duration_ms: prepare_ms,
        detail: format!("{data_sources} data, {fragments} fragments"),
    });
    log.step(&format!(
        "funnel  {data_sources} data, {fragments} fragments ({prepare_ms}ms)"
    ));

    let registry = Arc::new(registry);
    let route_rows = Mutex::new(Vec::with_capacity(prepared.len()));
    let warnings = Mutex::new(Vec::new());

    let t = Instant::now();
    let results: Vec<Result<EmitResult>> = prepared
        .par_iter()
        .map(|page| emit_prepared(opts, page, &registry, &warnings, &route_rows))
        .collect();
    let emit_ms = t.elapsed().as_millis();

    let mut outputs = Vec::new();
    for chunk in results {
        outputs.extend(chunk?.outputs);
    }
    phases.push(BuildPhase {
        name: "emit",
        duration_ms: emit_ms,
        detail: format!("{} pages", outputs.len()),
    });
    log.step(&format!("emit  {} pages ({emit_ms}ms)", outputs.len()));

    let mut warnings = warnings
        .into_inner()
        .map_err(|_| Error::at_file("<build>", "warnings mutex poisoned"))?;
    let mut routes = route_rows
        .into_inner()
        .map_err(|_| Error::at_file("<build>", "route summary mutex poisoned"))?;
    routes.sort_by(|a, b| a.route.cmp(&b.route));

    let mut assets_processed = 0;
    if opts.copy_assets {
        let t = Instant::now();
        let assets = emit::copy_static_assets(
            &opts.root,
            &opts.out_dir,
            &opts.asset_dirs,
            &opts.process,
        )?;
        let assets_ms = t.elapsed().as_millis();
        assets_processed = assets.processed;
        warnings.extend(assets.warnings);
        let detail = if opts.process.enabled {
            format!("{} processed", assets_processed)
        } else {
            format!("{} copied", assets.copied)
        };
        phases.push(BuildPhase {
            name: "assets",
            duration_ms: assets_ms,
            detail,
        });
        log.step(&format!("assets  {} ({assets_ms}ms)", phases.last().unwrap().detail));
    }

    let feed_pages: Vec<FeedPage<'_>> = prepared
        .iter()
        .map(|p| FeedPage {
            source: &p.source,
            data: &p.data,
            collection_id: feeds::collection_id_for_page(&p.doc),
        })
        .collect();

    let mut feed_detail = Vec::new();
    if opts.sitemap.enabled {
        feed_detail.push("sitemap");
    }
    if opts.rss.enabled {
        feed_detail.push("rss");
    }
    if !feed_detail.is_empty() {
        let t = Instant::now();
        warnings.extend(feeds::write_feeds(
            &opts.out_dir,
            &opts.site_url,
            &opts.sitemap,
            &opts.rss,
            &outputs,
            &feed_pages,
        )?);
        let feeds_ms = t.elapsed().as_millis();
        let detail = feed_detail.join(", ");
        phases.push(BuildPhase {
            name: "feeds",
            duration_ms: feeds_ms,
            detail: detail.to_string(),
        });
        log.step(&format!("feeds  {detail} ({feeds_ms}ms)"));
    }

    Ok(BuildReport {
        pages_written: outputs.len(),
        assets_processed,
        warnings,
        duration_ms: started.elapsed().as_millis(),
        outputs,
        phases,
        routes,
        sources,
        fragments,
        data_sources,
    })
}

fn prepare_pages(pages: &[PageSource], aliases: &AliasOptions) -> Result<(FragmentRegistry, Vec<PreparedPage>, usize)> {
    let mut registry = FragmentRegistry::new();
    let mut prepared = Vec::with_capacity(pages.len());
    let mut data_ids = HashSet::new();

    for page in pages {
        let file = page.path.display().to_string();
        let html = fs::read_to_string(&page.path).map_err(|e| Error::read(file.clone(), e))?;
        let doc = parse::parse_document(&html).map_err(|e| e.in_file(&file, &html))?;
        let dir = page.path.parent().unwrap_or_else(|| Path::new("."));
        let data = funnel::load_data_from_document(
            &doc,
            dir,
            registry.data_cache_mut(),
            aliases,
            Some((&file, &html)),
        )?;
        for id in data.keys() {
            data_ids.insert(id.clone());
        }
        registry.load_links_from_document(&doc, dir, aliases, Some((&file, &html)))?;
        prepared.push(PreparedPage {
            source: page.clone(),
            html,
            doc,
            data,
        });
    }
    Ok((registry, prepared, data_ids.len()))
}

fn emit_prepared(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    warnings: &Mutex<Vec<Diagnostic>>,
    route_rows: &Mutex<Vec<BuildRouteRow>>,
) -> Result<EmitResult> {
    let result = if let Some(rule) = opts.pagination_for(&page.source.route) {
        emit_paginated(opts, page, registry, rule, warnings)
    } else {
        match page.source.kind() {
            PageKind::Static => {
                let rendered = page.render(registry, None, &opts.emit, &opts.aliases, &opts.forms)?;
                let out = emit::out_path_for_route(&opts.out_dir, &page.source.route, None);
                emit::write_html(&out, &rendered)?;
                Ok(EmitResult {
                    outputs: vec![out],
                    route: page.route_row(1, PageKind::Static, false),
                })
            }
            PageKind::Collection => emit_collection(opts, page, registry, warnings),
        }
    }?;
    route_rows
        .lock()
        .map_err(|_| Error::at_file("<build>", "route summary mutex poisoned"))?
        .push(result.route.clone());
    Ok(result)
}

fn html_data_bind(doc: &Document) -> Option<String> {
    doc.children.iter().find_map(|n| match n {
        crate::parse::Node::Element(el) if el.name.eq_ignore_ascii_case("html") => {
            el.attr("data-bind").map(str::to_string)
        }
        _ => None,
    })
}

fn html_bind_needles(id: &str) -> [String; 2] {
    [
        format!("data-bind=\"{id}\""),
        format!("data-bind='{id}'"),
    ]
}

fn emit_paginated(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    rule: &PaginationRule,
    warnings: &Mutex<Vec<Diagnostic>>,
) -> Result<EmitResult> {
    let collection_id = html_data_bind(&page.doc).ok_or_else(|| {
        page.at(
            &["<html", "data-bind"],
            "paginated page needs data-bind on <html> pointing at a statica/data id",
        )
    })?;

    let needles = html_bind_needles(&collection_id);
    let needle_refs: Vec<&str> = needles.iter().map(String::as_str).collect();

    let param = page
        .source
        .params
        .first()
        .ok_or_else(|| {
            page.at(
                &needle_refs,
                format!(
                    "pagination route `{}` needs a [param] segment (e.g. blog/[page])",
                    page.source.route
                ),
            )
        })?
        .clone();

    if page.source.params.len() > 1 {
        return Err(page.at(
            &needle_refs,
            "pagination routes support a single [param] (the page number folder)",
        ));
    }

    let list = page.data.get(&collection_id).ok_or_else(|| {
        page.at(
            &needle_refs,
            format!(
                "missing data source id `{collection_id}` (no <script type=\"statica/data\" id=\"{collection_id}\">)"
            ),
        )
    })?;

    let items = match &list.value {
        Value::Array(a) => a.as_slice(),
        other => {
            return Err(page.at(
                &needle_refs,
                format!("pagination `{collection_id}` must be an array, got {other}"),
            ));
        }
    };

    let chunks = paginate::chunk_items(items, rule, &page.source.route, &param);
    if chunks.is_empty() {
        let mut w = warnings
            .lock()
            .map_err(|_| Error::at_file("<build>", "warnings mutex poisoned"))?;
        w.push(page.warn(
            &needle_refs,
            format!("pagination `{collection_id}` is empty — 0 pages emitted"),
        ));
        return Ok(EmitResult {
            outputs: Vec::new(),
            route: page.route_row(0, PageKind::Collection, true),
        });
    }

    let mut outs = Vec::with_capacity(chunks.len() + usize::from(rule.index));
    for chunk in &chunks {
        let rendered = page.render(registry, Some(&chunk.value), &opts.emit, &opts.aliases, &opts.forms)?;
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
            let rendered = page.render(registry, Some(&first.value), &opts.emit, &opts.aliases, &opts.forms)?;
            let index_route = paginate::index_route(&page.source.route, &param);
            let out = emit::out_path_for_route(&opts.out_dir, &index_route, None);
            emit::write_html(&out, &rendered)?;
            outs.push(out);
        }
    }

    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(
            chunks.len() + usize::from(rule.index),
            PageKind::Collection,
            true,
        ),
    })
}

fn emit_collection(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    warnings: &Mutex<Vec<Diagnostic>>,
) -> Result<EmitResult> {
    let collection_id = html_data_bind(&page.doc).ok_or_else(|| {
        page.at(
            &["<html", "data-bind"],
            "collection page needs data-bind on <html> pointing at a statica/data id",
        )
    })?;

    let needles = html_bind_needles(&collection_id);
    let needle_refs: Vec<&str> = needles.iter().map(String::as_str).collect();

    let list = page.data.get(&collection_id).ok_or_else(|| {
        page.at(
            &needle_refs,
            format!(
                "missing data source id `{collection_id}` (no <script type=\"statica/data\" id=\"{collection_id}\">)"
            ),
        )
    })?;

    let items = match &list.value {
        Value::Array(a) => a,
        other => {
            return Err(page.at(
                &needle_refs,
                format!("collection `{collection_id}` must be an array, got {other}"),
            ));
        }
    };

    if items.is_empty() {
        let mut w = warnings
            .lock()
            .map_err(|_| Error::at_file("<build>", "warnings mutex poisoned"))?;
        w.push(page.warn(
            &needle_refs,
            format!("collection `{collection_id}` is empty — 0 pages emitted"),
        ));
        return Ok(EmitResult {
            outputs: Vec::new(),
            route: page.route_row(0, PageKind::Collection, false),
        });
    }

    let param = page.source.params.first().ok_or_else(|| {
        page.at(&needle_refs, "collection without params")
    })?;

    let mut seen = HashSet::with_capacity(items.len());
    let mut outs = Vec::with_capacity(items.len());

    for item in items {
        let folder = funnel::field_as_str(item, param).ok_or_else(|| {
            page.at(
                &needle_refs,
                format!("collection item missing field `{param}` required by route `[{param}]`"),
            )
        })?;
        if !seen.insert(folder.clone()) {
            return Err(page.at(
                &needle_refs,
                format!("duplicate collection value for `[{param}]`: `{folder}`"),
            ));
        }
        let rendered = page.render(registry, Some(item), &opts.emit, &opts.aliases, &opts.forms)?;
        let out =
            emit::out_path_for_route(&opts.out_dir, &page.source.route, Some((param, &folder)));
        emit::write_html(&out, &rendered)?;
        outs.push(out);
    }
    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(count, PageKind::Collection, false),
    })
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
