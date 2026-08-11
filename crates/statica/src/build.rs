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
use rayon::ThreadPoolBuilder;
use serde::Serialize;
use serde_json::Value;

use crate::aliases::AliasOptions;
use crate::assets::AssetProcessOptions;
use crate::bind;
use crate::build_log::BuildLog;
use crate::discover::{self, PageFile, PageKind, PageSource, RouteParam};
use crate::emit;
use crate::error::{Error, Result};
use crate::feeds::{self, FeedPage, RssOptions, SitemapOptions};
use crate::fragment::FragmentRegistry;
use crate::funnel::{self, DataSource};
use crate::i18n::{self, I18nCatalogs, I18nOptions};
use crate::loc::Diagnostic;
use crate::manifest::{self, ManifestMeta};
use crate::minify::{self, MinifyOptions};
use crate::paginate::{self, PaginationRule};
use crate::parse::{self, Document};
use crate::render::RenderPlan;
use crate::FormsOptions;

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
    /// Scaffold `public/manifest.webmanifest` and inject PWA head tags.
    pub manifest: bool,
    /// List → `…/1/`, `…/2/`, … expansions.
    pub pagination: Vec<PaginationRule>,
    /// Asset optimize pipeline (off unless `enabled`; kinds are selectable).
    pub process: AssetProcessOptions,
    /// Final output minification (HTML, CSS, JS in `out_dir`).
    pub minify: MinifyOptions,
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
    /// Page rendering mode.
    pub render_mode: RenderMode,
    /// Maximum page-render worker threads when rendering in parallel (0 = rayon default).
    pub render_threads: usize,
}

impl BuildOptions {
    /// Pipeline defaults (no config file). Prefer the CLI for end-user settings.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            out_dir: root.join(".website"),
            copy_assets: true,
            site_url: String::new(),
            sitemap: SitemapOptions::default(),
            rss: RssOptions::default(),
            manifest: false,
            pagination: Vec::new(),
            process: AssetProcessOptions::default(),
            minify: MinifyOptions::default(),
            aliases: AliasOptions::default(),
            forms: FormsOptions::default(),
            i18n: I18nOptions::default(),
            clean: true,
            asset_dirs: vec!["public".into(), "assets".into(), "static".into()],
            ignore_dirs: vec![
                ".website".into(),
                "dist".into(),
                "target".into(),
                ".git".into(),
            ],
            root,
            verbose: false,
            render_mode: RenderMode::Auto,
            render_threads: 0,
        }
    }

    fn pagination_for(&self, route: &str) -> Option<&PaginationRule> {
        self.pagination
            .iter()
            .find(|rule| pagination_rule_matches_route(rule, route))
    }

    fn log(&self) -> BuildLog {
        BuildLog::new(self.verbose)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Auto,
    Serial,
    Parallel,
}

impl RenderMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Serial => "serial",
            Self::Parallel => "parallel",
        }
    }

    fn should_render_parallel(self) -> bool {
        matches!(self, Self::Auto | Self::Parallel)
    }
}

fn render_detail(mode: RenderMode, threads: usize) -> String {
    match (mode, threads) {
        (RenderMode::Serial, _) => "serial".to_string(),
        (RenderMode::Auto, 0) => "auto".to_string(),
        (RenderMode::Parallel, 0) => "parallel auto-threads".to_string(),
        (RenderMode::Auto, n) => format!("auto, {n} threads"),
        (RenderMode::Parallel, n) => format!("parallel, {n} threads"),
    }
}

fn pagination_rule_matches_route(rule: &PaginationRule, route: &str) -> bool {
    if !rule.route.is_empty() && rule.route == route {
        return true;
    }
    let root = pagination_rule_root(&rule.route);
    if root.is_empty() {
        return route.split('/').any(|segment| segment == "[page]");
    }
    let Some(tail) = route.strip_prefix(root) else {
        return false;
    };
    if !tail.starts_with('/') {
        return false;
    }
    tail.split('/').any(|segment| segment == "[page]")
}

fn pagination_rule_root(route: &str) -> &str {
    route.split_once("/[page]").map_or_else(
        || route.trim_matches('/'),
        |(root, _)| root.trim_matches('/'),
    )
}

fn pagination_listing_route(
    rule: &PaginationRule,
    page: &PreparedPage,
    page_param: &str,
) -> String {
    if rule.route.split('/').any(|segment| segment == "[page]") {
        return rule.route.clone();
    }
    let page_segment = format!("[{page_param}]");
    let mut parts = Vec::new();
    for segment in page.source.route.as_str().split('/') {
        parts.push(segment);
        if segment == page_segment {
            break;
        }
    }
    parts.join("/")
}

/// One timed pipeline step (for `--verbose` summary).
#[derive(Debug, Clone, Serialize)]
pub struct BuildPhase {
    pub name: &'static str,
    pub duration_ms: u128,
    pub detail: String,
}

/// Pages emitted for one discovered source route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildRouteKind {
    Static,
    Collection,
    Paginated,
}

impl From<PageKind> for BuildRouteKind {
    fn from(kind: PageKind) -> Self {
        match kind {
            PageKind::Static => Self::Static,
            PageKind::Collection => Self::Collection,
        }
    }
}

/// Pages emitted for one discovered source route.
#[derive(Debug, Clone, Serialize)]
pub struct BuildRouteRow {
    pub route: String,
    pub kind: BuildRouteKind,
    pub pages: usize,
}

#[derive(Debug, Default, Serialize)]
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

/// Which page sources should be emitted by this build pass.
///
/// Full builds may also run global output passes: redirects, default 404, asset
/// copy/process, feeds, and minification. Selected page builds are watch-mode
/// rebuilds for direct page edits and only re-emit the matching routes.
#[derive(Debug, Clone)]
enum BuildScope {
    Full,
    SelectedPages(PageSelection),
}

impl BuildScope {
    fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    fn contains_page(&self, file: &PageFile) -> bool {
        match self {
            Self::Full => true,
            Self::SelectedPages(paths) => paths.contains(file),
        }
    }

    fn discover_detail(&self, sources: usize) -> String {
        match self {
            Self::Full => format!("{sources} sources"),
            Self::SelectedPages(paths) => format!("{sources} sources, {} selected", paths.len()),
        }
    }
}

#[derive(Debug, Clone)]
enum RebuildPlan {
    /// All watched changes were ignored, so the existing output remains valid.
    Noop,
    /// Run the complete pipeline. `clean` is true when deleted inputs may have
    /// left stale generated files behind.
    Full { clean: bool },
    /// Re-emit exactly these discovered page source files.
    SelectedPages(PageSelection),
}

/// Watch events from generated or build-output directories do not invalidate
/// source output. Everything else is meaningful until proven otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangedPathKind {
    Ignored,
    Meaningful,
}

/// Result of mapping changed source paths onto page routes.
#[derive(Debug, Clone)]
enum PageRebuildSelection {
    Selected(PageSelection),
    RequiresFullBuild,
}

/// Whether page-only rebuilds can preserve output correctness for this config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageRebuildEligibility {
    Eligible,
    RequiresFullBuild,
}

struct PreparedPage {
    source: PageSource,
    html: String,
    doc: Document,
    render_plan: RenderPlan,
    data: std::collections::HashMap<String, DataSource>,
}

/// Explicit set of source pages selected for a scoped rebuild.
#[derive(Debug, Clone)]
struct PageSelection {
    files: HashSet<PageFile>,
}

impl PageSelection {
    fn new(files: HashSet<PageFile>) -> Self {
        Self { files }
    }

    fn len(&self) -> usize {
        self.files.len()
    }

    fn contains(&self, file: &PageFile) -> bool {
        self.files.contains(file)
    }
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
                    kind: crate::content::DataKind::Json,
                    path: PathBuf::from(format!("i18n:{key}")),
                    data: Arc::new(crate::content::DataSet::Json(value.clone())),
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
        self.source.path.as_path().display().to_string()
    }

    fn base_dir(&self) -> &Path {
        self.source
            .path
            .as_path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
    }

    fn active_locale<'a>(locale: Option<&'a str>, i18n: &'a I18nOptions) -> Option<&'a str> {
        locale.or({
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
        data_cache: &mut std::collections::HashMap<
            PathBuf,
            std::sync::Arc<crate::content::DataSet>,
        >,
        aliases: &AliasOptions,
        locale: Option<&str>,
        i18n_catalogs: &I18nCatalogs,
        i18n: &I18nOptions,
    ) -> Result<std::collections::HashMap<String, DataSource>> {
        let active_locale = Self::active_locale(locale, i18n);
        if funnel::document_has_dynamic_data(&self.doc) && active_locale.is_none() {
            return Err(self.at(
                &["rel=\"statica/data\"", "href=", "${"],
                "funnel href contains dynamic placeholders but no dynamic data context is available",
            ));
        }

        let mut data = self.data.clone();
        if let Some(loc) = active_locale.filter(|_| funnel::document_has_dynamic_data(&self.doc)) {
            let file = self.file();
            let dynamic_context = serde_json::json!({ "i18n": { "locale": loc } });
            let locale_data = funnel::load_dynamic_data_from_document(
                &self.doc,
                site_root,
                self.base_dir(),
                data_cache,
                aliases,
                &dynamic_context,
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
        aliases: &AliasOptions,
        forms: &FormsOptions,
        manifest: Option<&ManifestMeta>,
        locale: Option<&str>,
        i18n_catalogs: &I18nCatalogs,
        i18n: &I18nOptions,
        data_cache: &mut std::collections::HashMap<
            PathBuf,
            std::sync::Arc<crate::content::DataSet>,
        >,
    ) -> Result<String> {
        let file = self.file();
        let mut doc = self.doc.clone();
        let active_locale = Self::active_locale(locale, i18n);
        let catalog = locale.map(|loc| i18n_catalogs.for_locale(loc, i18n));
        if let Some(loc) = active_locale {
            i18n::set_html_lang(&mut doc, loc);
        }
        let page_data =
            self.resolve_page_data(site_root, data_cache, aliases, locale, i18n_catalogs, i18n)?;
        bind::render_page_document(
            registry,
            &doc,
            &self.render_plan,
            &self.source,
            current,
            &page_data,
            aliases,
            forms,
            manifest,
            locale,
            catalog.as_ref(),
            data_cache,
            Some((file.as_str(), self.html.as_str())),
        )
        .map_err(|e| e.in_file(&file, &self.html))
    }

    fn has_locale_param(&self, i18n: &I18nOptions) -> bool {
        i18n.route_has_locale(self.source.params.iter().map(RouteParam::as_str))
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

    fn route_row(&self, pages: usize, kind: impl Into<BuildRouteKind>) -> BuildRouteRow {
        BuildRouteRow {
            route: self.source.route.as_str().to_string(),
            kind: kind.into(),
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
        if funnel::data_link_has_dynamic_href(&self.doc, collection_id) {
            return true;
        }
        if !i18n.enabled {
            return false;
        }
        i18n.locales.iter().any(|loc| {
            i18n_catalogs
                .for_locale(loc, i18n)
                .get(collection_id)
                .is_some_and(Value::is_array)
        })
    }

    fn shared_collection_items(
        &self,
        collection_id: &str,
        needle_refs: &[&str],
    ) -> Result<Vec<Value>> {
        let list = self.data.get(collection_id).ok_or_else(|| {
            self.at(
                needle_refs,
                format!(
                    "missing data source id `{collection_id}` (no <link rel=\"statica/data\" id=\"{collection_id}\">)"
                ),
            )
        })?;
        list.array().ok_or_else(|| {
            let value = list.value();
            self.at(
                needle_refs,
                format!("collection `{collection_id}` must be an array, got {value}"),
            )
        })
    }
}

pub fn build(opts: &BuildOptions) -> Result<BuildReport> {
    build_scoped(opts, &BuildScope::Full)
}

fn build_scoped(opts: &BuildOptions, scope: &BuildScope) -> Result<BuildReport> {
    let started = Instant::now();
    let log = opts.log();
    let mut phases = Vec::new();
    opts.aliases.validate()?;

    if scope.is_full() && opts.clean && opts.out_dir.exists() {
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
        detail: scope.discover_detail(sources),
    });
    log.step(format!("discover  {sources} sources ({discover_ms}ms)"));

    let t = Instant::now();
    let i18n_catalogs = I18nCatalogs::load(&opts.root, &opts.i18n)?;
    let manifest_meta = if opts.manifest {
        let path = manifest::ensure_manifest_file(&opts.root)?;
        Some(manifest::read_manifest_meta(&path)?)
    } else {
        None
    };
    let extra_bind_roots = Vec::new();
    let (registry, prepared, data_sources) = prepare_pages(
        &pages,
        &opts.root,
        &opts.aliases,
        &opts.forms,
        manifest_meta.as_ref(),
        &extra_bind_roots,
    )?;
    let prepare_ms = t.elapsed().as_millis();
    let fragments = registry.len();
    phases.push(BuildPhase {
        name: "funnel",
        duration_ms: prepare_ms,
        detail: format!("{data_sources} data, {fragments} fragments"),
    });
    log.step(format!(
        "funnel  {data_sources} data, {fragments} fragments ({prepare_ms}ms)"
    ));

    let registry = Arc::new(registry);
    let selected_prepared: Vec<&PreparedPage> = prepared
        .iter()
        .filter(|page| scope.contains_page(&page.source.path))
        .collect();
    let route_rows = Mutex::new(Vec::with_capacity(selected_prepared.len()));
    let warnings = Mutex::new(Vec::new());

    let t = Instant::now();
    let render_mode = opts.render_mode;
    let render_emit_parallel = || {
        selected_prepared
            .par_iter()
            .map(|page| {
                emit_prepared(
                    opts,
                    page,
                    &registry,
                    &i18n_catalogs,
                    manifest_meta.as_ref(),
                    &warnings,
                    &route_rows,
                )
            })
            .collect::<Vec<Result<EmitResult>>>()
    };
    let results: Vec<Result<EmitResult>> = match render_mode {
        RenderMode::Auto | RenderMode::Parallel if opts.render_threads == 0 => {
            render_emit_parallel()
        }
        RenderMode::Serial => selected_prepared
            .iter()
            .map(|page| {
                emit_prepared(
                    opts,
                    page,
                    &registry,
                    &i18n_catalogs,
                    manifest_meta.as_ref(),
                    &warnings,
                    &route_rows,
                )
            })
            .collect(),
        RenderMode::Auto | RenderMode::Parallel => ThreadPoolBuilder::new()
            .num_threads(opts.render_threads)
            .build()
            .map_err(|e| Error::at_file("<build>", e.to_string()))?
            .install(render_emit_parallel),
    };
    let emit_ms = t.elapsed().as_millis();

    let render_outputs = results
        .into_iter()
        .map(|result| result.map(|chunk| chunk.outputs))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let parallel_detail = render_detail(render_mode, opts.render_threads);
    phases.push(BuildPhase {
        name: "emit",
        duration_ms: emit_ms,
        detail: format!("{} pages, render {parallel_detail}", render_outputs.len()),
    });
    log.step(format!(
        "emit  {} pages, render {parallel_detail} ({emit_ms}ms)",
        render_outputs.len()
    ));

    let mut warnings = warnings
        .into_inner()
        .map_err(|_| Error::at_file("<build>", "warnings mutex poisoned"))?;
    let mut routes = route_rows
        .into_inner()
        .map_err(|_| Error::at_file("<build>", "route summary mutex poisoned"))?;

    let root_redirect =
        if scope.is_full() && i18n::should_emit_root_redirect(&opts.i18n, &pages, &opts.out_dir) {
            let redirect = opts.out_dir.join("index.html");
            emit::write_html(
                &redirect,
                &i18n::root_redirect_html(&opts.i18n.default_locale),
            )?;
            routes.push(BuildRouteRow {
                route: String::new(),
                kind: BuildRouteKind::Static,
                pages: 1,
            });
            log.step(format!("redirect  / → /{}/", opts.i18n.default_locale));
            Some(redirect)
        } else {
            None
        };

    let outputs = render_outputs
        .into_iter()
        .chain(root_redirect)
        .collect::<Vec<_>>();

    if scope.is_full() {
        ensure_default_404(&opts.out_dir)?;
    }

    routes.sort_by(|a, b| a.route.cmp(&b.route));

    let mut assets_processed = 0;
    if scope.is_full() && opts.copy_assets {
        let t = Instant::now();
        let assets =
            emit::copy_static_assets(&opts.root, &opts.out_dir, &opts.asset_dirs, &opts.process)?;
        let assets_ms = t.elapsed().as_millis();
        assets_processed = assets.processed;
        warnings.extend(assets.warnings);
        let detail = if opts.process.enabled {
            format!("{assets_processed} processed")
        } else {
            format!("{} copied", assets.copied)
        };
        phases.push(BuildPhase {
            name: "assets",
            duration_ms: assets_ms,
            detail: detail.clone(),
        });
        log.step(format!("assets  {detail} ({assets_ms}ms)"));

        if opts.process.enabled && opts.process.images && !assets.images.is_empty() {
            let t = Instant::now();
            let responsive_html = crate::images::apply_responsive_html(
                &opts.out_dir,
                &assets.images,
                &opts.process.image,
            );
            let img_ms = t.elapsed().as_millis();
            warnings.extend(responsive_html.warnings);
            if responsive_html.images_rewritten > 0 {
                let img_count = responsive_html.images_rewritten;
                phases.push(BuildPhase {
                    name: "images",
                    duration_ms: img_ms,
                    detail: format!("{img_count} img tags"),
                });
                log.step(format!("images  {img_count} img tags ({img_ms}ms)"));
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
    if scope.is_full() && opts.sitemap.enabled {
        feed_detail.push("sitemap");
    }
    if scope.is_full() && opts.rss.enabled {
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
            detail: detail.clone(),
        });
        log.step(format!("feeds  {detail} ({feeds_ms}ms)"));
    }

    if scope.is_full() && opts.minify.enabled {
        let t = Instant::now();
        let minified = minify::minify_output_dir(&opts.out_dir, &opts.minify);
        let minify_ms = t.elapsed().as_millis();
        warnings.extend(minified.warnings);
        phases.push(BuildPhase {
            name: "minify",
            duration_ms: minify_ms,
            detail: format!("{} files", minified.files),
        });
        log.step(format!("minify  {} files ({minify_ms}ms)", minified.files));
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

fn ensure_default_404(out_dir: &Path) -> Result<()> {
    let flat = out_dir.join("404.html");
    let nested = out_dir.join("404").join("index.html");
    if flat.exists() || nested.exists() {
        return Ok(());
    }
    emit::write_html(&nested, default_404_html())?;
    Ok(())
}

fn default_404_html() -> &'static str {
    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>404 Not Found</title>
    <style>
      body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 10vh auto; padding: 0 1rem; line-height: 1.5; }
      a { color: #0f766e; }
    </style>
  </head>
  <body>
    <h1>404 Not Found</h1>
    <p>The page you are looking for does not exist.</p>
    <p><a href="/">Return home</a></p>
  </body>
</html>
"#
}

fn prepare_pages(
    pages: &[PageSource],
    site_root: &Path,
    aliases: &AliasOptions,
    forms: &FormsOptions,
    manifest: Option<&ManifestMeta>,
    extra_bind_roots: &[String],
) -> Result<(FragmentRegistry, Vec<PreparedPage>, usize)> {
    let mut registry = FragmentRegistry::with_extra_bind_roots(site_root, extra_bind_roots);
    let mut prepared = Vec::with_capacity(pages.len());
    let mut data_ids = HashSet::new();

    for page in pages {
        let file = page.path.as_path().display().to_string();
        let html =
            fs::read_to_string(page.path.as_path()).map_err(|e| Error::read(file.clone(), e))?;
        let mut doc = parse::parse_document(&html).map_err(|e| e.in_file(&file, &html))?;
        bind::transform_page_styles(&mut doc.children);
        let dir = page
            .path
            .as_path()
            .parent()
            .unwrap_or_else(|| Path::new("."));
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
        bind::validate_collection_page_binds(
            &doc,
            page.kind(),
            page.params
                .iter()
                .any(|param| param.as_str() != i18n::LOCALE_PARAM),
            funnel::BindSource {
                file: &file,
                source: &html,
            },
            extra_bind_roots,
        )?;
        bind::validate_page_binds(
            &doc,
            funnel::BindSource {
                file: &file,
                source: &html,
            },
            extra_bind_roots,
        )?;
        crate::aliases::resolve_paths_in_document(&mut doc, aliases, Some((&file, &html)))?;
        crate::font::expand_font_links(&mut doc, Some((&file, &html)))?;
        if let Some(meta) = manifest {
            crate::manifest::inject_manifest_tags(&mut doc, meta);
        }
        crate::forms::wire_forms_in_document(&mut doc, forms, Some((&file, &html)))?;
        prepared.push(PreparedPage {
            source: page.clone(),
            html,
            render_plan: RenderPlan::compile_document(&doc),
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
    manifest: Option<&ManifestMeta>,
    warnings: &Mutex<Vec<Diagnostic>>,
    route_rows: &Mutex<Vec<BuildRouteRow>>,
) -> Result<EmitResult> {
    let result = if page.locale_only(&opts.i18n) {
        emit_locales(opts, page, registry, i18n_catalogs, manifest)
    } else if let Some(rule) = opts.pagination_for(page.source.route.as_str()) {
        if page.has_locale_param(&opts.i18n) {
            emit_locale_paginated(
                opts,
                page,
                registry,
                i18n_catalogs,
                rule,
                manifest,
                warnings,
            )
        } else {
            emit_paginated(
                opts,
                page,
                registry,
                i18n_catalogs,
                rule,
                manifest,
                warnings,
                None,
            )
        }
    } else if page.has_locale_param(&opts.i18n) && page.source.kind() == PageKind::Collection {
        emit_locale_collection(opts, page, registry, i18n_catalogs, manifest, warnings)
    } else {
        match page.source.kind() {
            PageKind::Static => {
                let mut data_cache = std::collections::HashMap::new();
                let rendered = page.render(
                    registry,
                    &opts.root,
                    None,
                    &opts.aliases,
                    &opts.forms,
                    manifest,
                    None,
                    i18n_catalogs,
                    &opts.i18n,
                    &mut data_cache,
                )?;
                let out = emit::out_path_for_route(&opts.out_dir, page.source.route.as_str(), None);
                emit::write_html(&out, &rendered)?;
                Ok(EmitResult {
                    outputs: vec![out],
                    route: page.route_row(1, PageKind::Static),
                })
            }
            PageKind::Collection => {
                emit_collection(opts, page, registry, i18n_catalogs, manifest, warnings)
            }
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
    manifest: Option<&ManifestMeta>,
) -> Result<EmitResult> {
    let locales = &opts.i18n.locales;
    let mut outs = Vec::with_capacity(locales.len());
    let mut data_cache = std::collections::HashMap::new();
    for loc in locales {
        let rendered = page.render(
            registry,
            &opts.root,
            None,
            &opts.aliases,
            &opts.forms,
            manifest,
            Some(loc.as_str()),
            i18n_catalogs,
            &opts.i18n,
            &mut data_cache,
        )?;
        let out = emit::out_path_for_route(
            &opts.out_dir,
            page.source.route.as_str(),
            Some((i18n::LOCALE_PARAM, loc)),
        );
        emit::write_html(&out, &rendered)?;
        outs.push(out);
    }
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(locales.len(), PageKind::Static),
    })
}

fn collection_param(params: &[RouteParam]) -> Result<&str> {
    params
        .iter()
        .find(|p| p.as_str() != i18n::LOCALE_PARAM)
        .map(RouteParam::as_str)
        .ok_or_else(|| {
            Error::at_file(
                "<build>",
                "locale collection route needs a param besides [locale] (e.g. [locale]/posts/[slug])",
            )
        })
}

fn html_data_source(doc: &Document) -> Option<String> {
    bind::html_collection_id(doc)
}

fn html_source_needles(id: &str) -> [String; 2] {
    bind::collection_needles(id)
}

fn emit_locale_paginated(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    rule: &PaginationRule,
    manifest: Option<&ManifestMeta>,
    warnings: &Mutex<Vec<Diagnostic>>,
) -> Result<EmitResult> {
    let collection_id = html_data_source(&page.doc).ok_or_else(|| {
        page.at(
            &["<html", "data-bind"],
            "paginated page needs data-bind on <html> (data-bind=\"id\" or data-bind=\"{…}\")",
        )
    })?;
    let needles = html_source_needles(&collection_id);
    let needle_refs: Vec<&str> = needles.iter().map(String::as_str).collect();
    let param = pagination_param(page, &needle_refs)?;
    let listing_route = pagination_listing_route(rule, page, &param);
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
            let chunks = paginate::chunk_items(&items, rule, &listing_route, &param);
            let localized: Vec<_> = chunks
                .iter()
                .map(|chunk| paginate::apply_locale_to_chunk(chunk, loc))
                .collect();
            if localized.is_empty() {
                push_empty_pagination_warning(page, warnings, &collection_id, &needle_refs)?;
                continue;
            }
            emit_pagination_chunks(
                opts,
                page,
                registry,
                rule,
                &localized,
                &param,
                Some(loc.as_str()),
                i18n_catalogs,
                manifest,
                &mut data_cache,
                &mut outs,
            )?;
        }
    } else {
        let items = page.shared_collection_items(&collection_id, &needle_refs)?;
        let chunks = paginate::chunk_items(&items, rule, &listing_route, &param);
        if chunks.is_empty() {
            push_empty_pagination_warning(page, warnings, &collection_id, &needle_refs)?;
            return Ok(EmitResult {
                outputs: Vec::new(),
                route: page.route_row(0, BuildRouteKind::Paginated),
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
                manifest,
                &mut data_cache,
                &mut outs,
            )?;
        }
    }

    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(count, BuildRouteKind::Paginated),
    })
}

fn pagination_param(page: &PreparedPage, needle_refs: &[&str]) -> Result<String> {
    if page.source.params.iter().any(|p| p.as_str() == "page") {
        return Ok("page".into());
    }
    page.source
        .params
        .iter()
        .find(|p| p.as_str() != i18n::LOCALE_PARAM)
        .map(|p| p.as_str().to_string())
        .ok_or_else(|| {
            page.at(
                needle_refs,
                format!(
                    "pagination route `{}` needs a [page] segment (e.g. blog/[page])",
                    page.source.route.as_str()
                ),
            )
        })
}

fn pagination_item_param<'a>(page: &'a PreparedPage, page_param: &str) -> Option<&'a str> {
    page.source
        .params
        .iter()
        .map(RouteParam::as_str)
        .find(|param| *param != i18n::LOCALE_PARAM && *param != page_param)
}

fn pagination_items_for_locale(
    page: &PreparedPage,
    site_root: &Path,
    collection_id: &str,
    needle_refs: &[&str],
    data_cache: &mut std::collections::HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    aliases: &AliasOptions,
    locale: Option<&str>,
    i18n_catalogs: &I18nCatalogs,
    i18n: &I18nOptions,
) -> Result<Vec<Value>> {
    let page_data =
        page.resolve_page_data(site_root, data_cache, aliases, locale, i18n_catalogs, i18n)?;
    let list = page_data.get(collection_id).ok_or_else(|| {
        page.at(
            needle_refs,
            format!(
                "missing data source id `{collection_id}` (no <link rel=\"statica/data\" id=\"{collection_id}\">)"
            ),
        )
    })?;
    list.array().ok_or_else(|| {
        let value = list.value();
        page.at(
            needle_refs,
            format!("pagination `{collection_id}` must be an array, got {value}"),
        )
    })
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
    manifest: Option<&ManifestMeta>,
    data_cache: &mut std::collections::HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    outs: &mut Vec<PathBuf>,
) -> Result<()> {
    let item_param = pagination_item_param(page, param);
    if page
        .source
        .params
        .iter()
        .filter(|p| p.as_str() != i18n::LOCALE_PARAM && p.as_str() != param)
        .count()
        > 1
    {
        return Err(page.at(
            &["[page]"],
            "pagination routes support at most one item param besides [page] and [locale]",
        ));
    }
    match item_param {
        Some(item_param) => {
            for chunk in chunks {
                emit_paginated_item_chunk(
                    opts,
                    page,
                    registry,
                    chunk,
                    param,
                    item_param,
                    locale,
                    i18n_catalogs,
                    manifest,
                    outs,
                )?;
            }
        }
        None => {
            emit_paginated_listing_chunks(
                opts,
                page,
                registry,
                chunks,
                param,
                locale,
                i18n_catalogs,
                manifest,
                outs,
            )?;
        }
    }

    if rule.index && item_param.is_none() {
        if let Some(first) = chunks.first() {
            let index_route = paginate::index_route(page.source.route.as_str(), param);
            let rendered = page.render(
                registry,
                &opts.root,
                Some(&first.value),
                &opts.aliases,
                &opts.forms,
                manifest,
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

fn emit_paginated_item_chunk(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    chunk: &paginate::PageChunk,
    page_param: &str,
    item_param: &str,
    locale: Option<&str>,
    i18n_catalogs: &I18nCatalogs,
    manifest: Option<&ManifestMeta>,
    outs: &mut Vec<PathBuf>,
) -> Result<()> {
    let items = chunk
        .value
        .get(paginate::PaginationField::Items.as_str())
        .and_then(Value::as_array)
        .ok_or_else(|| page.at(&["page.pagination.items"], "pagination chunk missing items"))?;
    let tasks = items
        .iter()
        .map(|item| {
            let folder = funnel::field_as_str(item, item_param).ok_or_else(|| {
                page.at(
                    &[item_param],
                    format!(
                        "pagination item missing field `{item_param}` required by route `[{item_param}]`"
                    ),
                )
            })?;
            Ok((item.clone(), folder))
        })
        .collect::<Result<Vec<_>>>()?;

    let render = |(item, folder): &(Value, String)| {
        let mut data_cache = std::collections::HashMap::new();
        let mut ctx = serde_json::Map::new();
        ctx.insert(
            crate::context::CanonicalRoot::Item.as_str().into(),
            item.clone(),
        );
        ctx.insert(
            crate::context::CanonicalPageField::Pagination
                .as_str()
                .into(),
            chunk.value.clone(),
        );
        ctx.insert(page_param.into(), Value::String(chunk.page.clone()));
        let ctx = Value::Object(ctx);
        let rendered = page.render(
            registry,
            &opts.root,
            Some(&ctx),
            &opts.aliases,
            &opts.forms,
            manifest,
            locale,
            i18n_catalogs,
            &opts.i18n,
            &mut data_cache,
        )?;
        let out = if let Some(loc) = locale {
            emit::out_path_for_route_replacements(
                &opts.out_dir,
                page.source.route.as_str(),
                &[
                    (i18n::LOCALE_PARAM, loc),
                    (page_param, &chunk.page),
                    (item_param, folder),
                ],
            )
        } else {
            emit::out_path_for_route_replacements(
                &opts.out_dir,
                page.source.route.as_str(),
                &[(page_param, &chunk.page), (item_param, folder)],
            )
        };
        emit::write_html(&out, &rendered)?;
        Ok(out)
    };
    let rendered = if opts.render_mode.should_render_parallel() {
        tasks
            .par_iter()
            .map(render)
            .collect::<Vec<Result<PathBuf>>>()
    } else {
        tasks.iter().map(render).collect::<Vec<Result<PathBuf>>>()
    };
    for out in rendered {
        outs.push(out?);
    }
    Ok(())
}

fn emit_paginated_listing_chunks(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    chunks: &[paginate::PageChunk],
    param: &str,
    locale: Option<&str>,
    i18n_catalogs: &I18nCatalogs,
    manifest: Option<&ManifestMeta>,
    outs: &mut Vec<PathBuf>,
) -> Result<()> {
    let render = |chunk: &paginate::PageChunk| {
        let mut data_cache = std::collections::HashMap::new();
        let rendered = page.render(
            registry,
            &opts.root,
            Some(&chunk.value),
            &opts.aliases,
            &opts.forms,
            manifest,
            locale,
            i18n_catalogs,
            &opts.i18n,
            &mut data_cache,
        )?;
        let out = if let Some(loc) = locale {
            emit::out_path_for_route_replacements(
                &opts.out_dir,
                page.source.route.as_str(),
                &[(i18n::LOCALE_PARAM, loc), (param, &chunk.page)],
            )
        } else {
            emit::out_path_for_route(
                &opts.out_dir,
                page.source.route.as_str(),
                Some((param, &chunk.page)),
            )
        };
        emit::write_html(&out, &rendered)?;
        Ok(out)
    };
    let rendered = if opts.render_mode.should_render_parallel() {
        chunks
            .par_iter()
            .map(render)
            .collect::<Vec<Result<PathBuf>>>()
    } else {
        chunks.iter().map(render).collect::<Vec<Result<PathBuf>>>()
    };
    for out in rendered {
        outs.push(out?);
    }
    Ok(())
}

fn emit_collection_items(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    manifest: Option<&ManifestMeta>,
    param: &str,
    items: &[Value],
) -> Result<Vec<PathBuf>> {
    let render = |item: &Value| {
        let folder = funnel::field_as_str(item, param).ok_or_else(|| {
            page.at(
                &[param],
                format!("collection item missing field `{param}` required by route `[{param}]`"),
            )
        })?;
        let mut data_cache = std::collections::HashMap::new();
        let rendered = page.render(
            registry,
            &opts.root,
            Some(item),
            &opts.aliases,
            &opts.forms,
            manifest,
            None,
            i18n_catalogs,
            &opts.i18n,
            &mut data_cache,
        )?;
        let out = emit::out_path_for_route(
            &opts.out_dir,
            page.source.route.as_str(),
            Some((param, &folder)),
        );
        emit::write_html(&out, &rendered)?;
        Ok(out)
    };
    let rendered = if opts.render_mode.should_render_parallel() {
        items
            .par_iter()
            .map(render)
            .collect::<Vec<Result<PathBuf>>>()
    } else {
        items.iter().map(render).collect::<Vec<Result<PathBuf>>>()
    };
    rendered.into_iter().collect()
}

fn emit_locale_collection_items_parallel(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    manifest: Option<&ManifestMeta>,
    param: &str,
    items: &[Value],
    loc: &str,
    needle_refs: &[&str],
    seen: &mut HashSet<String>,
) -> Result<Vec<PathBuf>> {
    let tasks = items
        .iter()
        .map(|item| {
            let folder = funnel::field_as_str(item, param).ok_or_else(|| {
                page.at(
                    needle_refs,
                    format!(
                        "collection item missing field `{param}` required by route `[{param}]`"
                    ),
                )
            })?;
            let key = format!("{loc}:{folder}");
            if !seen.insert(key) {
                return Err(page.at(
                    needle_refs,
                    format!("duplicate collection value for `[{param}]`: `{folder}`"),
                ));
            }
            Ok((item.clone(), folder))
        })
        .collect::<Result<Vec<_>>>()?;
    let render = |(item, folder): &(Value, String)| {
        let mut data_cache = std::collections::HashMap::new();
        let rendered = page.render(
            registry,
            &opts.root,
            Some(item),
            &opts.aliases,
            &opts.forms,
            manifest,
            Some(loc),
            i18n_catalogs,
            &opts.i18n,
            &mut data_cache,
        )?;
        let out = emit::out_path_for_route_replacements(
            &opts.out_dir,
            page.source.route.as_str(),
            &[(i18n::LOCALE_PARAM, loc), (param, folder)],
        );
        emit::write_html(&out, &rendered)?;
        Ok(out)
    };
    let rendered = if opts.render_mode.should_render_parallel() {
        tasks
            .par_iter()
            .map(render)
            .collect::<Vec<Result<PathBuf>>>()
    } else {
        tasks.iter().map(render).collect::<Vec<Result<PathBuf>>>()
    };
    rendered.into_iter().collect()
}

fn emit_paginated(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    rule: &PaginationRule,
    manifest: Option<&ManifestMeta>,
    warnings: &Mutex<Vec<Diagnostic>>,
    locale: Option<&str>,
) -> Result<EmitResult> {
    let collection_id = html_data_source(&page.doc).ok_or_else(|| {
        page.at(
            &["<html", "data-bind"],
            "paginated page needs data-bind on <html> (data-bind=\"id\" or data-bind=\"{…}\")",
        )
    })?;

    let needles = html_source_needles(&collection_id);
    let needle_refs: Vec<&str> = needles.iter().map(String::as_str).collect();
    let param = pagination_param(page, &needle_refs)?;
    let listing_route = pagination_listing_route(rule, page, &param);
    let mut data_cache = std::collections::HashMap::new();
    let items = if locale.is_some()
        || page.collection_varies_by_locale(&collection_id, i18n_catalogs, &opts.i18n)
    {
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
    };

    let chunks = paginate::chunk_items(&items, rule, &listing_route, &param);
    if chunks.is_empty() {
        push_empty_pagination_warning(page, warnings, &collection_id, &needle_refs)?;
        return Ok(EmitResult {
            outputs: Vec::new(),
            route: page.route_row(0, BuildRouteKind::Paginated),
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
        manifest,
        &mut data_cache,
        &mut outs,
    )?;

    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(count, BuildRouteKind::Paginated),
    })
}

fn emit_collection(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    manifest: Option<&ManifestMeta>,
    warnings: &Mutex<Vec<Diagnostic>>,
) -> Result<EmitResult> {
    let collection_id = html_data_source(&page.doc).ok_or_else(|| {
        page.at(
            &["<html", "data-bind"],
            "collection page needs data-bind on <html> (data-bind=\"id\" or data-bind=\"{…}\")",
        )
    })?;

    let needles = html_source_needles(&collection_id);
    let needle_refs: Vec<&str> = needles.iter().map(String::as_str).collect();

    let page_data = page.resolve_page_data(
        &opts.root,
        &mut std::collections::HashMap::new(),
        &opts.aliases,
        None,
        i18n_catalogs,
        &opts.i18n,
    )?;
    let list = page_data.get(&collection_id).ok_or_else(|| {
        page.at(
            &needle_refs,
            format!(
                "missing data source id `{collection_id}` (no <link rel=\"statica/data\" id=\"{collection_id}\">)"
            ),
        )
    })?;

    let items = list.array().ok_or_else(|| {
        let value = list.value();
        page.at(
            &needle_refs,
            format!("collection `{collection_id}` must be an array, got {value}"),
        )
    })?;

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
            route: page.route_row(0, PageKind::Collection),
        });
    }

    let param = page
        .source
        .params
        .first()
        .ok_or_else(|| page.at(&needle_refs, "collection without params"))?;
    let param = param.as_str();

    let mut seen = HashSet::with_capacity(items.len());

    for item in &items {
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
    }
    let outs = emit_collection_items(opts, page, registry, i18n_catalogs, manifest, param, &items)?;
    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(count, PageKind::Collection),
    })
}

fn emit_locale_collection(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    manifest: Option<&ManifestMeta>,
    warnings: &Mutex<Vec<Diagnostic>>,
) -> Result<EmitResult> {
    let collection_id = html_data_source(&page.doc).ok_or_else(|| {
        page.at(
            &["<html", "data-bind"],
            "collection page needs data-bind on <html> (data-bind=\"id\" or data-bind=\"{…}\")",
        )
    })?;

    let needles = html_source_needles(&collection_id);
    let needle_refs: Vec<&str> = needles.iter().map(String::as_str).collect();

    let param =
        collection_param(&page.source.params).map_err(|e| page.at(&needle_refs, e.to_string()))?;

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
                        "missing data source id `{collection_id}` (no <link rel=\"statica/data\" id=\"{collection_id}\">)"
                    ),
                )
            })?;
            let items = list.array().ok_or_else(|| {
                let value = list.value();
                page.at(
                    &needle_refs,
                    format!("collection `{collection_id}` must be an array, got {value}"),
                )
            })?;
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
            outs.extend(emit_locale_collection_items_parallel(
                opts,
                page,
                registry,
                i18n_catalogs,
                manifest,
                param,
                &items,
                loc,
                &needle_refs,
                &mut seen,
            )?);
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
                route: page.route_row(0, PageKind::Collection),
            });
        }
        let mut seen = HashSet::new();
        for loc in &opts.i18n.locales {
            outs.extend(emit_locale_collection_items_parallel(
                opts,
                page,
                registry,
                i18n_catalogs,
                manifest,
                param,
                &items,
                loc,
                &needle_refs,
                &mut seen,
            )?);
        }
    }

    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        route: page.route_row(count, PageKind::Collection),
    })
}

pub fn rebuild_paths(opts: &BuildOptions, changed: &[PathBuf]) -> Result<BuildReport> {
    match plan_rebuild(opts, changed)? {
        RebuildPlan::Noop => Ok(BuildReport::default()),
        RebuildPlan::SelectedPages(paths) => {
            let mut incremental = opts.clone();
            incremental.clean = false;
            build_scoped(&incremental, &BuildScope::SelectedPages(paths))
        }
        RebuildPlan::Full { clean } => {
            let mut full = opts.clone();
            full.clean = clean;
            build(&full)
        }
    }
}

fn plan_rebuild(opts: &BuildOptions, changed: &[PathBuf]) -> Result<RebuildPlan> {
    let meaningful = meaningful_changed_paths(opts, changed);

    match meaningful.as_slice() {
        [] if changed.is_empty() => Ok(RebuildPlan::Full { clean: true }),
        [] => Ok(RebuildPlan::Noop),
        paths => match page_rebuild_eligibility(opts) {
            PageRebuildEligibility::Eligible => match selected_changed_pages(opts, paths)? {
                PageRebuildSelection::Selected(selected) => {
                    Ok(RebuildPlan::SelectedPages(selected))
                }
                PageRebuildSelection::RequiresFullBuild => Ok(RebuildPlan::Full {
                    clean: paths.iter().any(|path| !path.exists()),
                }),
            },
            PageRebuildEligibility::RequiresFullBuild => Ok(RebuildPlan::Full {
                clean: paths.iter().any(|path| !path.exists()),
            }),
        },
    }
}

fn meaningful_changed_paths(opts: &BuildOptions, changed: &[PathBuf]) -> Vec<PathBuf> {
    changed
        .iter()
        .filter_map(|path| {
            let path = normalize_changed_path(&opts.root, path);
            match changed_path_kind(opts, &path) {
                ChangedPathKind::Ignored => None,
                ChangedPathKind::Meaningful => Some(path),
            }
        })
        .collect()
}

fn changed_path_kind(opts: &BuildOptions, path: &Path) -> ChangedPathKind {
    if path.starts_with(&opts.out_dir) {
        return ChangedPathKind::Ignored;
    }
    if path
        .components()
        .any(|component| component.as_os_str() == std::ffi::OsStr::new("target"))
    {
        return ChangedPathKind::Ignored;
    }
    ChangedPathKind::Meaningful
}

fn page_rebuild_eligibility(opts: &BuildOptions) -> PageRebuildEligibility {
    if opts.minify.enabled || opts.process.enabled {
        return PageRebuildEligibility::RequiresFullBuild;
    }
    PageRebuildEligibility::Eligible
}

fn selected_changed_pages(
    opts: &BuildOptions,
    changed: &[PathBuf],
) -> Result<PageRebuildSelection> {
    let pages = discover::discover_pages(&opts.root, &opts.ignore_dirs)?;
    let mut selected = HashSet::new();

    for path in changed {
        if !path.exists() || path.file_name().is_none_or(|name| name != "index.html") {
            return Ok(PageRebuildSelection::RequiresFullBuild);
        }
        let Some(page) = pages
            .iter()
            .find(|page| same_path(page.path.as_path(), path))
        else {
            return Ok(PageRebuildSelection::RequiresFullBuild);
        };
        selected.insert(page.path.clone());
    }

    if selected.is_empty() {
        Ok(PageRebuildSelection::RequiresFullBuild)
    } else {
        Ok(PageRebuildSelection::Selected(PageSelection::new(selected)))
    }
}

fn normalize_changed_path(root: &Path, path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    path.canonicalize().unwrap_or(path)
}

fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
