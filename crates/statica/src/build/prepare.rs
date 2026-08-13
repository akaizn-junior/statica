//! Page preparation: read, parse, load data/fragments, validate, and compile render plans.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::aliases::AliasOptions;
use crate::bind;
use crate::discover::PageSource;
use crate::error::{Error, Result};
use crate::fragment::FragmentRegistry;
use crate::funnel;
use crate::i18n;
use crate::manifest::ManifestMeta;
use crate::parse;
use crate::render::RenderPlan;
use crate::FormsOptions;

use super::PreparedPage;

pub(super) fn prepare_pages(
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
