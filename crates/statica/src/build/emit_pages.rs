//! Page route emission: static, localized, collection, and paginated outputs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rayon::prelude::*;
use serde_json::Value;

use crate::aliases::AliasOptions;
use crate::bind;
use crate::discover::{PageKind, RouteParam};
use crate::emit;
use crate::error::{Error, Result};
use crate::fragment::FragmentRegistry;
use crate::funnel;
use crate::i18n::{self, I18nCatalogs, I18nOptions};
use crate::loc::Diagnostic;
use crate::manifest::ManifestMeta;
use crate::paginate::{self, PaginationRule};
use crate::parse::Document;
use crate::search;

use super::output::write_rendered_html;
use super::page::PreparedPage;
use super::{pagination_listing_route, BuildOptions, BuildRouteKind, BuildRouteRow, EmitResult};

#[derive(Debug, Clone, Copy)]
enum PageEmissionPlan<'a> {
    Locales,
    LocalePaginated { rule: &'a PaginationRule },
    Paginated { rule: &'a PaginationRule },
    LocaleCollection,
    Static,
    Collection,
}

impl<'a> PageEmissionPlan<'a> {
    fn for_page(opts: &'a BuildOptions, page: &PreparedPage) -> Self {
        match (
            page.locale_only(&opts.i18n),
            opts.pagination_for(page.source.route.as_str()),
            page.has_locale_param(&opts.i18n),
            page.source.kind(),
        ) {
            (true, _, _, _) => Self::Locales,
            (_, Some(rule), true, _) => Self::LocalePaginated { rule },
            (_, Some(rule), false, _) => Self::Paginated { rule },
            (_, None, true, PageKind::Collection) => Self::LocaleCollection,
            (_, None, _, PageKind::Static) => Self::Static,
            (_, None, _, PageKind::Collection) => Self::Collection,
        }
    }
}

pub(super) fn emit_prepared(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    manifest: Option<&ManifestMeta>,
    warnings: &Mutex<Vec<Diagnostic>>,
    route_rows: &Mutex<Vec<BuildRouteRow>>,
) -> Result<EmitResult> {
    let result = match PageEmissionPlan::for_page(opts, page) {
        PageEmissionPlan::Locales => emit_locales(opts, page, registry, i18n_catalogs, manifest),
        PageEmissionPlan::LocalePaginated { rule } => emit_locale_paginated(
            opts,
            page,
            registry,
            i18n_catalogs,
            rule,
            manifest,
            warnings,
        ),
        PageEmissionPlan::Paginated { rule } => emit_paginated(
            opts,
            page,
            registry,
            i18n_catalogs,
            rule,
            manifest,
            warnings,
            None,
        ),
        PageEmissionPlan::LocaleCollection => {
            emit_locale_collection(opts, page, registry, i18n_catalogs, manifest, warnings)
        }
        PageEmissionPlan::Static => {
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
            write_rendered_html(opts, &out, &rendered)?;
            Ok(EmitResult {
                outputs: vec![out],
                search_entries: Vec::new(),
                route: page.route_row(1, PageKind::Static),
            })
        }
        PageEmissionPlan::Collection => {
            emit_collection(opts, page, registry, i18n_catalogs, manifest, warnings)
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
        write_rendered_html(opts, &out, &rendered)?;
        outs.push(out);
    }
    Ok(EmitResult {
        outputs: outs,
        search_entries: Vec::new(),
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
    let mut search_entries = Vec::new();

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
                &mut search_entries,
                &collection_id,
            )?;
        }
    } else {
        let items = page.shared_collection_items(&collection_id, &needle_refs)?;
        let chunks = paginate::chunk_items(&items, rule, &listing_route, &param);
        if chunks.is_empty() {
            push_empty_pagination_warning(page, warnings, &collection_id, &needle_refs)?;
            return Ok(EmitResult {
                outputs: Vec::new(),
                search_entries: Vec::new(),
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
                &mut search_entries,
                &collection_id,
            )?;
        }
    }

    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        search_entries,
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
    search_entries: &mut Vec<search::SearchEntry>,
    collection_id: &str,
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
                    search_entries,
                    collection_id,
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
            write_rendered_html(opts, &out, &rendered)?;
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
    search_entries: &mut Vec<search::SearchEntry>,
    collection_id: &str,
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
        write_rendered_html(opts, &out, &rendered)?;
        let url = search_url_for_output(&opts.out_dir, &out);
        Ok::<_, Error>((out, search::entry_for_item(item, url, collection_id)))
    };
    let rendered = if opts.render_mode.should_render_parallel() {
        tasks.par_iter().map(render).collect::<Vec<_>>()
    } else {
        tasks.iter().map(render).collect::<Vec<_>>()
    };
    for out in rendered {
        let (out, entry) = out?;
        outs.push(out);
        search_entries.push(entry);
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
        write_rendered_html(opts, &out, &rendered)?;
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
    collection_id: &str,
    param: &str,
    items: &[Value],
) -> Result<(Vec<PathBuf>, Vec<search::SearchEntry>)> {
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
        write_rendered_html(opts, &out, &rendered)?;
        let url = search_url_for_output(&opts.out_dir, &out);
        Ok::<_, Error>((out, search::entry_for_item(item, url, collection_id)))
    };
    let rendered = if opts.render_mode.should_render_parallel() {
        items.par_iter().map(render).collect::<Vec<_>>()
    } else {
        items.iter().map(render).collect::<Vec<_>>()
    };
    let mut outs = Vec::new();
    let mut entries = Vec::new();
    for rendered in rendered {
        let (out, entry) = rendered?;
        outs.push(out);
        entries.push(entry);
    }
    Ok((outs, entries))
}

fn emit_locale_collection_items_parallel(
    opts: &BuildOptions,
    page: &PreparedPage,
    registry: &FragmentRegistry,
    i18n_catalogs: &I18nCatalogs,
    manifest: Option<&ManifestMeta>,
    collection_id: &str,
    param: &str,
    items: &[Value],
    loc: &str,
    needle_refs: &[&str],
    seen: &mut HashSet<String>,
) -> Result<(Vec<PathBuf>, Vec<search::SearchEntry>)> {
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
        write_rendered_html(opts, &out, &rendered)?;
        let url = search_url_for_output(&opts.out_dir, &out);
        Ok::<_, Error>((out, search::entry_for_item(item, url, collection_id)))
    };
    let rendered = if opts.render_mode.should_render_parallel() {
        tasks.par_iter().map(render).collect::<Vec<_>>()
    } else {
        tasks.iter().map(render).collect::<Vec<_>>()
    };
    let mut outs = Vec::new();
    let mut entries = Vec::new();
    for rendered in rendered {
        let (out, entry) = rendered?;
        outs.push(out);
        entries.push(entry);
    }
    Ok((outs, entries))
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
            search_entries: Vec::new(),
            route: page.route_row(0, BuildRouteKind::Paginated),
        });
    }

    let mut outs = Vec::with_capacity(chunks.len() + usize::from(rule.index));
    let mut search_entries = Vec::new();
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
        &mut search_entries,
        &collection_id,
    )?;

    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        search_entries,
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
            search_entries: Vec::new(),
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
    let (outs, search_entries) = emit_collection_items(
        opts,
        page,
        registry,
        i18n_catalogs,
        manifest,
        &collection_id,
        param,
        &items,
    )?;
    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        search_entries,
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
    let mut search_entries = Vec::new();
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
            let (loc_outs, loc_entries) = emit_locale_collection_items_parallel(
                opts,
                page,
                registry,
                i18n_catalogs,
                manifest,
                &collection_id,
                param,
                &items,
                loc,
                &needle_refs,
                &mut seen,
            )?;
            outs.extend(loc_outs);
            search_entries.extend(loc_entries);
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
                search_entries: Vec::new(),
                route: page.route_row(0, PageKind::Collection),
            });
        }
        let mut seen = HashSet::new();
        for loc in &opts.i18n.locales {
            let (loc_outs, loc_entries) = emit_locale_collection_items_parallel(
                opts,
                page,
                registry,
                i18n_catalogs,
                manifest,
                &collection_id,
                param,
                &items,
                loc,
                &needle_refs,
                &mut seen,
            )?;
            outs.extend(loc_outs);
            search_entries.extend(loc_entries);
        }
    }

    let count = outs.len();
    Ok(EmitResult {
        outputs: outs,
        search_entries,
        route: page.route_row(count, PageKind::Collection),
    })
}

fn search_url_for_output(out_dir: &Path, path: &Path) -> String {
    let Ok(rel) = path.strip_prefix(out_dir) else {
        return "/".into();
    };
    if rel == Path::new("index.html") {
        return "/".into();
    }
    let mut parts = rel
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect::<Vec<_>>();
    if parts.last() == Some(&"index.html") {
        parts.pop();
    }
    format!("/{}/", parts.join("/"))
}
