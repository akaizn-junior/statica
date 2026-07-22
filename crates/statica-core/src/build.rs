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
use crate::i18n::{self, I18nCatalogs, I18nOptions};
use crate::loc::Diagnostic;
use crate::minify::{self, MinifyOptions};
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
    /// Final output minification (HTML, CSS, JS in `out_dir`).
    pub minify: MinifyOptions,
    /// What to strip / tidy when writing HTML.
    pub emit: EmitOptions,
    /// Path / URL aliases for authoring (`[aliases]` in statica.toml).
    pub aliases: AliasOptions,
    /// Static form wiring (`[forms]` in statica.toml).
    pub forms: FormsOptions,
    /// Locale catalogs (`[i18n]` in statica.toml).
    pub i18n: I18nOptions,
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
            minify: MinifyOptions::default(),
            emit: EmitOptions::default(),
            aliases: AliasOptions::default(),
            forms: FormsOptions::default(),
            i18n: I18nOptions::default(),
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

/// Overlay locale catalog arrays onto page data (i18n-driven `data-each` sources).
fn merge_i18n_data(
    page_data: &std::collections::HashMap<String, DataSource>,
    catalog: Option<&Value>,
) -> std::collections::HashMap<String, DataSource> {
    let Some(Value::Object(map)) = catalog else {
        return page_data.clone();
    };
    let mut merged = page_data.clone();
    for (key, value) in map {
        if value.is_array() {
            merged.insert(
                key.clone(),
                DataSource {
                    id: key.clone(),
                    path: PathBuf::from(format!("i18n:{key}")),
                    value: value.clone(),
                },
            );
        }
    }
    merged
}

struct EmitResult {
    outputs: Vec<PathBuf>,
    route: BuildRouteRow,
}

impl PreparedPage {
    fn file(&self) -> String {
        self.source.path.display().to_string()
    }

    fn base_dir(&self) -> &Path {
        self.source.path.parent().unwrap_or_else(|| Path::new("."))
    }

    fn active_locale<'a>(&self, locale: Option<&'a str>, i18n: &'a I18nOptions) -> Option<&'a str> {
        locale.or_else(|| {
            if i18n.enabled {
                Some(i18n.default_locale.as_str())
            } else {
                None
            }
        })
    }

    fn resolve_page_data(
        &self,
        site_root: &Path,
        data_cache: &mut std::collections::HashMap<PathBuf, Value>,
        aliases: &AliasOptions,
        locale: Option<&str>,
        i18n_catalogs: &I18nCatalogs,
        i18n: &I18nOptions,
    ) -> Result<std::collections::HashMap<String, DataSource>> {
        let active_locale = self.active_locale(locale, i18n);
        if funnel::document_has_locale_data(&self.doc) && active_locale.is_none() {
            return Err(self.at(
                &["type=\"statica/data\"", i18n::LOCALE_SRC_TOKEN],
                format!(
                    "funnel src contains `{}` but i18n is disabled — enable [i18n] or remove the locale token",
                    i18n::LOCALE_SRC_TOKEN
                ),
            ));
        }

        let mut data = self.data.clone();
        if let Some(loc) = active_locale.filter(|_| funnel::document_has_locale_data(&self.doc)) {
            let file = self.file();
            let locale_data = funnel::load_locale_data_from_document(
                &self.doc,
                site_root,
                self.base_dir(),
                data_cache,
                aliases,
                loc,
                Some((file.as_str(), self.html.as_str())),
            )
            .map_err(|e| e.in_file(&file, &self.html))?;
            for (id, source) in locale_data {
                data.insert(id, source);
            }
        }

        let catalog = active_locale.map(|loc| i18n_catalogs.for_locale(loc, i18n));
        Ok(merge_i18n_data(&data, catalog.as_ref()))
    }

    fn render(
        &self,
        registry: &FragmentRegistry,
        site_root: &Path,
        current: Option<&Value>,
        emit: &EmitOptions,
        aliases: &AliasOptions,
        forms: &FormsOptions,
        locale: Option<&str>,
        i18n_catalogs: &I18nCatalogs,
        i18n: &I18nOptions,
        data_cache: &mut std::collections::HashMap<PathBuf, Value>,
    ) -> Result<String> {
        let file = self.file();
        let mut doc = self.doc.clone();
        let active_locale = self.active_locale(locale, i18n);
        let catalog = locale.map(|loc| i18n_catalogs.for_locale(loc, i18n));
        if let Some(loc) = active_locale {
            i18n::set_html_lang(&mut doc, loc);
        }
        let page_data = self.resolve_page_data(site_root, data_cache, aliases, locale, i18n_catalogs, i18n)?;
        bind::render_page_document(
            registry,
            &doc,
            current,
            &page_data,
            emit,
            aliases,
            forms,
            locale,
            catalog.as_ref(),
            data_cache,
            Some((file.as_str(), self.html.as_str())),
        )
        .map_err(|e| e.in_file(&file, &self.html))
    }

    fn has_locale_param(&self, i18n: &I18nOptions) -> bool {
        i18n.route_has_locale(self.source.params.iter().map(String::as_str))
    }

    fn locale_only(&self, i18n: &I18nOptions) -> bool {
        self.has_locale_param(i18n) && self.source.params.len() == 1
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

    /// Whether a paginated/collection data source differs per locale.
    fn collection_varies_by_locale(
        &self,
        collection_id: &str,
        i18n_catalogs: &I18nCatalogs,
        i18n: &I18nOptions,
    ) -> bool {
        if funnel::data_script_has_locale_token(&self.doc, collection_id) {
            return true;
        }
        if !i18n.enabled {
            return false;
        }
        i18n.locales.iter().any(|loc| {
            i18n_catalogs
                .for_locale(loc, i18n)
                .get(collection_id)
                .is_some_and(|v| v.is_array())
        })
    }

    fn shared_collection_items(
        &self,
        collection_id: &str,
        needle_refs: &[&str],
    ) -> Result<&[Value]> {
        let list = self.data.get(collection_id).ok_or_else(|| {
            self.at(
                needle_refs,
                format!(
                    "missing data source id `{collection_id}` (no <script type=\"statica/data\" id=\"{collection_id}\">)"
                ),
            )
        })?;
        match &list.value {
            Value::Array(a) => Ok(a.as_slice()),
            other => Err(self.at(
                needle_refs,
                format!("collection `{collection_id}` must be an array, got {other}"),
            )),
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
    let (registry, prepared, data_sources) = prepare_pages(&pages, &opts.root, &opts.aliases)?;
    let i18n_catalogs = I18nCatalogs::load(&opts.root, &opts.i18n)?;
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
        .map(|page| emit_prepared(opts, page, &registry, &i18n_catalogs, &warnings, &route_rows))
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

        if opts.process.enabled && opts.process.images && !assets.images.is_empty() {
            let t = Instant::now();
            let (img_count, img_warnings) = crate::images::apply_responsive_html(
                &opts.out_dir,
                &assets.images,
                &opts.process.image,
            )?;
            let img_ms = t.elapsed().as_millis();
            warnings.extend(img_warnings);
            if img_count > 0 {
                phases.push(BuildPhase {
                    name: "images",
                    duration_ms: img_ms,
                    detail: format!("{img_count} img tags"),
                });
                log.step(&format!("images  {img_count} img tags ({img_ms}ms)"));
            }
        }
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

    if opts.minify.enabled {
        let t = Instant::now();
        let minified = minify::minify_output_dir(&opts.out_dir, &opts.minify)?;
        let minify_ms = t.elapsed().as_millis();
        warnings.extend(minified.warnings);
        phases.push(BuildPhase {
            name: "minify",
            duration_ms: minify_ms,
            detail: format!("{} files", minified.files),
        });
        log.step(&format!(
            "minify  {} files ({minify_ms}ms)",
            minified.files
        ));
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

fn prepare_pages(
    pages: &[PageSource],
    site_root: &Path,
    aliases: &AliasOptions,
) -> Result<(FragmentRegistry, Vec<PreparedPage>, usize)> {
    let mut registry = FragmentRegistry::new(site_root);
    let mut prepared = Vec::with_capacity(pages.len());
    let mut data_ids = HashSet::new();

    for page in pages {
        let file = page.path.display().to_string();
        let html = fs::read_to_string(&page.path).map_err(|e| Error::read(file.clone(), e))?;
        let doc = parse::parse_document(&html).map_err(|e| e.in_file(&file, &html))?;
        let dir = page.path.parent().unwrap_or_else(|| Path::new("."));
        let data = funnel::load_data_from_document(
            &doc,
            site_root,
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
    i18n_catalogs: &I18nCatalogs,
    warnings: &Mutex<Vec<Diagnostic>>,
    route_rows: &Mutex<Vec<BuildRouteRow>>,
) -> Result<EmitResult> {
    let result = if page.locale_only(&opts.i18n) {
        emit_locales(opts, page, registry, i18n_catalogs)
    } else if let Some(rule) = opts.pagination_for(&page.source.route) {
        if page.has_locale_param(&opts.i18n) {
            emit_locale_paginated(opts, page, registry, i18n_catalogs, rule, warnings)
        } else {
            emit_paginated(opts, page, registry, i18n_catalogs, rule, warnings, None)
        }
    } else if page.has_locale_param(&opts.i18n) && page.source.kind() == PageKind::Collection {
        emit_locale_collection(opts, page, registry, i18n_catalogs, warnings)
    } else {
        match page.source.kind() {
            PageKind::Static => {
                let mut data_cache = std::collections::HashMap::new();
                let rendered = page.render(
                    registry,
                    &opts.root,
                    None,
                    &opts.emit,
                    &opts.aliases,
                    &opts.forms,
                    None,
                    i18n_catalogs,
                    &opts.i18n,
                    &mut data_cache,
                )?;
                let out = emit::out_path_for_route(&opts.out_dir, &page.source.route, None);
                emit::write_html(&out, &rendered)?;
                Ok(EmitResult {
                    outputs: vec![out],
                    route: page.route_row(1, PageKind::Static, false),
                })
            }
            PageKind::Collection => emit_collection(opts, page, registry, i18n_catalogs, warnings),
        }
    }?;
    route_rows
        .lock()
        .map_err(|_| Error::at_file("<build>", "route summary mutex poisoned"))?
        .push(result.route.clone());
    Ok(result)
}

fn emit_locales(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
) -> Result<EmitResult> {
    let locales = &opts.i18n.locales;
    let mut outs = Vec::with_capacity(locales.len());
    let mut data_cache = std::collections::HashMap::new();
    for loc in locales {
        let ctx = i18n::locale_bind_context(loc);
        let rendered = page.render(
            registry,
            &opts.root,
            Some(&ctx),
            &opts.emit,
            &opts.aliases,
            &opts.forms,
            Some(loc.as_str()),
            i18n_catalogs,
            &opts.i18n,
            &mut data_cache,
        )?;
        let out = emit::out_path_for_route(
            &opts.out_dir,
            &page.source.route,
            Some((i18n::LOCALE_PARAM, loc)),
        );
        emit::write_html(&out, &rendered)?;
        outs.push(out);
    }
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(locales.len(), PageKind::Static, false),
    })
}

fn collection_param<'a>(params: &'a [String]) -> Result<&'a str> {
    params
        .iter()
        .find(|p| *p != i18n::LOCALE_PARAM)
        .map(String::as_str)
        .ok_or_else(|| {
            Error::at_file(
                "<build>",
                "locale collection route needs a param besides [locale] (e.g. [locale]/posts/[slug])",
            )
        })
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

fn emit_locale_paginated(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
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
    let param = pagination_param(page, &needle_refs)?;
    let mut data_cache = std::collections::HashMap::new();
    let mut outs = Vec::new();

    if page.collection_varies_by_locale(&collection_id, i18n_catalogs, &opts.i18n) {
        for loc in &opts.i18n.locales {
            let items = pagination_items_for_locale(
                page,
                &opts.root,
                &collection_id,
                &needle_refs,
                &mut data_cache,
                &opts.aliases,
                Some(loc.as_str()),
                i18n_catalogs,
                &opts.i18n,
            )?;
            let chunks = paginate::chunk_items(&items, rule, &page.source.route, &param);
            if chunks.is_empty() {
                push_empty_pagination_warning(page, warnings, &collection_id, &needle_refs)?;
                continue;
            }
            emit_pagination_chunks(
                opts,
                page,
                registry,
                rule,
                &chunks,
                &param,
                Some(loc.as_str()),
                i18n_catalogs,
                &mut data_cache,
                &mut outs,
            )?;
        }
    } else {
        let items = page.shared_collection_items(&collection_id, &needle_refs)?;
        let chunks = paginate::chunk_items(items, rule, &page.source.route, &param);
        if chunks.is_empty() {
            push_empty_pagination_warning(page, warnings, &collection_id, &needle_refs)?;
            return Ok(EmitResult {
                outputs: Vec::new(),
                route: page.route_row(0, PageKind::Collection, true),
            });
        }
        for loc in &opts.i18n.locales {
            let localized: Vec<_> = chunks
                .iter()
                .map(|chunk| paginate::apply_locale_to_chunk(chunk, loc))
                .collect();
            emit_pagination_chunks(
                opts,
                page,
                registry,
                rule,
                &localized,
                &param,
                Some(loc.as_str()),
                i18n_catalogs,
                &mut data_cache,
                &mut outs,
            )?;
        }
    }

    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(count, PageKind::Collection, true),
    })
}

fn pagination_param(page: &PreparedPage, needle_refs: &[&str]) -> Result<String> {
    let param = page
        .source
        .params
        .iter()
        .find(|p| *p != i18n::LOCALE_PARAM)
        .ok_or_else(|| {
            page.at(
                needle_refs,
                format!(
                    "pagination route `{}` needs a [param] segment (e.g. blog/[page])",
                    page.source.route
                ),
            )
        })?
        .clone();
    let pagination_params: Vec<_> = page
        .source
        .params
        .iter()
        .filter(|p| *p != i18n::LOCALE_PARAM)
        .collect();
    if pagination_params.len() > 1 {
        return Err(page.at(
            needle_refs,
            "pagination routes support a single [param] besides [locale] (the page number folder)",
        ));
    }
    Ok(param)
}

fn pagination_items_for_locale(
    page: &PreparedPage,
    site_root: &Path,
    collection_id: &str,
    needle_refs: &[&str],
    data_cache: &mut std::collections::HashMap<PathBuf, Value>,
    aliases: &AliasOptions,
    locale: Option<&str>,
    i18n_catalogs: &I18nCatalogs,
    i18n: &I18nOptions,
) -> Result<Vec<Value>> {
    let page_data = page.resolve_page_data(site_root, data_cache, aliases, locale, i18n_catalogs, i18n)?;
    let list = page_data.get(collection_id).ok_or_else(|| {
        page.at(
            needle_refs,
            format!(
                "missing data source id `{collection_id}` (no <script type=\"statica/data\" id=\"{collection_id}\">)"
            ),
        )
    })?;
    match &list.value {
        Value::Array(a) => Ok(a.clone()),
        other => Err(page.at(
            needle_refs,
            format!("pagination `{collection_id}` must be an array, got {other}"),
        )),
    }
}

fn push_empty_pagination_warning(
    page: &PreparedPage,
    warnings: &Mutex<Vec<Diagnostic>>,
    collection_id: &str,
    needle_refs: &[&str],
) -> Result<()> {
    let mut w = warnings
        .lock()
        .map_err(|_| Error::at_file("<build>", "warnings mutex poisoned"))?;
    w.push(page.warn(
        needle_refs,
        format!("pagination `{collection_id}` is empty — 0 pages emitted"),
    ));
    Ok(())
}

fn emit_pagination_chunks(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    rule: &PaginationRule,
    chunks: &[paginate::PageChunk],
    param: &str,
    locale: Option<&str>,
    i18n_catalogs: &I18nCatalogs,
    data_cache: &mut std::collections::HashMap<PathBuf, Value>,
    outs: &mut Vec<PathBuf>,
) -> Result<()> {
    for chunk in chunks {
        let ctx = locale.map(|loc| i18n::merge_locale_into(&chunk.value, loc));
        let rendered = page.render(
            registry,
            &opts.root,
            ctx.as_ref().or(Some(&chunk.value)),
            &opts.emit,
            &opts.aliases,
            &opts.forms,
            locale,
            i18n_catalogs,
            &opts.i18n,
            data_cache,
        )?;
        let out = if let Some(loc) = locale {
            emit::out_path_for_route_replacements(
                &opts.out_dir,
                &page.source.route,
                &[(i18n::LOCALE_PARAM, loc), (param, &chunk.page)],
            )
        } else {
            emit::out_path_for_route(
                &opts.out_dir,
                &page.source.route,
                Some((param, &chunk.page)),
            )
        };
        emit::write_html(&out, &rendered)?;
        outs.push(out);
    }

    if rule.index {
        if let Some(first) = chunks.first() {
            let ctx = locale.map(|loc| i18n::merge_locale_into(&first.value, loc));
            let index_route = paginate::index_route(&page.source.route, param);
            let rendered = page.render(
                registry,
                &opts.root,
                ctx.as_ref().or(Some(&first.value)),
                &opts.emit,
                &opts.aliases,
                &opts.forms,
                locale,
                i18n_catalogs,
                &opts.i18n,
                data_cache,
            )?;
            let out = if let Some(loc) = locale {
                emit::out_path_for_route_replacements(
                    &opts.out_dir,
                    &index_route,
                    &[(i18n::LOCALE_PARAM, loc)],
                )
            } else {
                emit::out_path_for_route(&opts.out_dir, &index_route, None)
            };
            emit::write_html(&out, &rendered)?;
            outs.push(out);
        }
    }
    Ok(())
}

fn emit_paginated(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    rule: &PaginationRule,
    warnings: &Mutex<Vec<Diagnostic>>,
    locale: Option<&str>,
) -> Result<EmitResult> {
    let collection_id = html_data_bind(&page.doc).ok_or_else(|| {
        page.at(
            &["<html", "data-bind"],
            "paginated page needs data-bind on <html> pointing at a statica/data id",
        )
    })?;

    let needles = html_bind_needles(&collection_id);
    let needle_refs: Vec<&str> = needles.iter().map(String::as_str).collect();
    let param = pagination_param(page, &needle_refs)?;
    let mut data_cache = std::collections::HashMap::new();
    let items = if locale.is_some() || page.collection_varies_by_locale(&collection_id, i18n_catalogs, &opts.i18n) {
        pagination_items_for_locale(
            page,
            &opts.root,
            &collection_id,
            &needle_refs,
            &mut data_cache,
            &opts.aliases,
            locale,
            i18n_catalogs,
            &opts.i18n,
        )?
    } else {
        page.shared_collection_items(&collection_id, &needle_refs)?
            .to_vec()
    };

    let chunks = paginate::chunk_items(&items, rule, &page.source.route, &param);
    if chunks.is_empty() {
        push_empty_pagination_warning(page, warnings, &collection_id, &needle_refs)?;
        return Ok(EmitResult {
            outputs: Vec::new(),
            route: page.route_row(0, PageKind::Collection, true),
        });
    }

    let mut outs = Vec::with_capacity(chunks.len() + usize::from(rule.index));
    emit_pagination_chunks(
        opts,
        page,
        registry,
        rule,
        &chunks,
        &param,
        locale,
        i18n_catalogs,
        &mut data_cache,
        &mut outs,
    )?;

    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(count, PageKind::Collection, true),
    })
}

fn emit_collection(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
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

    let mut data_cache = std::collections::HashMap::new();
    let page_data = page.resolve_page_data(
        &opts.root,
        &mut data_cache,
        &opts.aliases,
        None,
        i18n_catalogs,
        &opts.i18n,
    )?;
    let list = page_data.get(&collection_id).ok_or_else(|| {
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
        let rendered = page.render(
            registry,
            &opts.root,
            Some(item),
            &opts.emit,
            &opts.aliases,
            &opts.forms,
            None,
            i18n_catalogs,
            &opts.i18n,
            &mut data_cache,
        )?;
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

fn emit_locale_collection(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
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

    let param = collection_param(&page.source.params).map_err(|e| {
        page.at(
            &needle_refs,
            e.to_string(),
        )
    })?;

    let mut outs = Vec::new();
    let mut data_cache = std::collections::HashMap::new();
    let varies = page.collection_varies_by_locale(&collection_id, i18n_catalogs, &opts.i18n);

    if varies {
        for loc in &opts.i18n.locales {
            let page_data = page.resolve_page_data(
                &opts.root,
                &mut data_cache,
                &opts.aliases,
                Some(loc.as_str()),
                i18n_catalogs,
                &opts.i18n,
            )?;
            let list = page_data.get(&collection_id).ok_or_else(|| {
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
                continue;
            }
            let mut seen = HashSet::new();
            emit_locale_collection_items(
                opts,
                page,
                registry,
                i18n_catalogs,
                param,
                items,
                loc,
                &needle_refs,
                &mut data_cache,
                &mut outs,
                &mut seen,
            )?;
        }
    } else {
        let items = page.shared_collection_items(&collection_id, &needle_refs)?;
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
        let mut seen = HashSet::new();
        for loc in &opts.i18n.locales {
            emit_locale_collection_items(
                opts,
                page,
                registry,
                i18n_catalogs,
                param,
                items,
                loc,
                &needle_refs,
                &mut data_cache,
                &mut outs,
                &mut seen,
            )?;
        }
    }

    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(count, PageKind::Collection, false),
    })
}

fn emit_locale_collection_items(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    param: &str,
    items: &[Value],
    loc: &str,
    needle_refs: &[&str],
    data_cache: &mut std::collections::HashMap<PathBuf, Value>,
    outs: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
) -> Result<()> {
    for item in items {
        let folder = funnel::field_as_str(item, param).ok_or_else(|| {
            page.at(
                needle_refs,
                format!("collection item missing field `{param}` required by route `[{param}]`"),
            )
        })?;
        let key = format!("{loc}:{folder}");
        if !seen.insert(key) {
            return Err(page.at(
                needle_refs,
                format!("duplicate collection value for `[{param}]`: `{folder}`"),
            ));
        }
        let ctx = i18n::merge_locale_into(item, loc);
        let rendered = page.render(
            registry,
            &opts.root,
            Some(&ctx),
            &opts.emit,
            &opts.aliases,
            &opts.forms,
            Some(loc),
            i18n_catalogs,
            &opts.i18n,
            data_cache,
        )?;
        let out = emit::out_path_for_route_replacements(
            &opts.out_dir,
            &page.source.route,
            &[(i18n::LOCALE_PARAM, loc), (param, &folder)],
        );
        emit::write_html(&out, &rendered)?;
        outs.push(out);
    }
    Ok(())
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
