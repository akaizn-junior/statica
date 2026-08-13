//! Build orchestration: discover → pre/parse → funnel → bind → scope → emit.
//!
//! [`build`] is the main entry. Collection pages expand 1:N via
//! [`emit_collection`]; routes listed in [`BuildOptions::pagination`] expand via
//! [`emit_paginated`] (see [`crate::paginate`]).

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::build_log::BuildLog;
use crate::discover;
use crate::emit;
use crate::error::{Error, Result};
use crate::feeds::{self, FeedPage};
use crate::i18n::{self, I18nCatalogs};
use crate::manifest;
use crate::minify;
use crate::paginate::PaginationRule;
use crate::search;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

mod emit_pages;
mod output;
mod page;
mod prepare;
mod rebuild;
mod types;

use emit_pages::emit_prepared;
use output::ensure_default_404;
use page::PreparedPage;
use prepare::prepare_pages;
pub use rebuild::rebuild_paths;
use rebuild::BuildScope;
use types::render_detail;
pub use types::{BuildOptions, BuildPhase, BuildReport, BuildRouteKind, BuildRouteRow, RenderMode};

impl BuildOptions {
    fn pagination_for(&self, route: &str) -> Option<&PaginationRule> {
        self.pagination
            .iter()
            .find(|rule| pagination_rule_matches_route(rule, route))
    }

    fn log(&self) -> BuildLog {
        BuildLog::new(self.verbose)
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

struct EmitResult {
    outputs: Vec<PathBuf>,
    search_entries: Vec<search::SearchEntry>,
    route: BuildRouteRow,
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

    let emitted = results.into_iter().collect::<Result<Vec<_>>>()?;
    let render_outputs = emitted
        .iter()
        .flat_map(|chunk| chunk.outputs.iter().cloned())
        .collect::<Vec<_>>();
    let mut search_entries = Vec::new();
    for chunk in emitted {
        search_entries.extend(chunk.search_entries);
    }
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

    let search_controls = if scope.is_full() {
        search::count_controls_in_outputs(&outputs)
    } else {
        0
    };
    if scope.is_full() && (opts.search.enabled || search_controls > 0) {
        let t = Instant::now();
        search::write_index(&opts.out_dir, &outputs, &opts.search, search_entries)?;
        if search_controls > 0 {
            search::write_runtime(&opts.out_dir)?;
        }
        let search_ms = t.elapsed().as_millis();
        let detail = if search_controls > 0 {
            format!("index, {search_controls} control(s)")
        } else {
            "index".into()
        };
        phases.push(BuildPhase {
            name: "search",
            duration_ms: search_ms,
            detail: detail.clone(),
        });
        log.step(format!("search  {detail} ({search_ms}ms)"));
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
