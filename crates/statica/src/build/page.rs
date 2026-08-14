//! Prepared page model and render-time data resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::aliases::AliasOptions;
use crate::bind;
use crate::discover::{PageSource, RouteParam};
use crate::error::{Error, Result};
use crate::fragment::FragmentRegistry;
use crate::funnel::{self, DataSource};
use crate::i18n::{self, I18nCatalogs, I18nOptions};
use crate::loc::Diagnostic;
use crate::manifest::ManifestMeta;
use crate::parse::Document;
use crate::render::RenderPlan;
use crate::tokens::{missing_data_source_message, rel_double_quoted_data};
use crate::FormsOptions;

use super::{BuildRouteKind, BuildRouteRow};

pub(super) struct PreparedPage {
    pub(super) source: PageSource,
    pub(super) html: String,
    pub(super) doc: Document,
    pub(super) render_plan: RenderPlan,
    pub(super) data: HashMap<String, DataSource>,
}

impl PreparedPage {
    pub(super) fn file(&self) -> String {
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

    pub(super) fn resolve_page_data(
        &self,
        site_root: &Path,
        data_cache: &mut HashMap<PathBuf, Arc<crate::content::DataSet>>,
        aliases: &AliasOptions,
        locale: Option<&str>,
        i18n_catalogs: &I18nCatalogs,
        i18n: &I18nOptions,
    ) -> Result<HashMap<String, DataSource>> {
        let active_locale = Self::active_locale(locale, i18n);
        if funnel::document_has_dynamic_data(&self.doc) && active_locale.is_none() {
            let data_rel = rel_double_quoted_data();
            return Err(self.at(
                &[data_rel.as_str(), "href=", "${"],
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

    pub(super) fn render(
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
        data_cache: &mut HashMap<PathBuf, Arc<crate::content::DataSet>>,
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

    pub(super) fn has_locale_param(&self, i18n: &I18nOptions) -> bool {
        i18n.route_has_locale(self.source.params.iter().map(RouteParam::as_str))
    }

    pub(super) fn locale_only(&self, i18n: &I18nOptions) -> bool {
        self.has_locale_param(i18n) && self.source.params.len() == 1
    }

    pub(super) fn at(&self, needles: &[&str], message: impl Into<String>) -> Error {
        Error::at(&self.file(), &self.html, needles, message)
    }

    pub(super) fn warn(&self, needles: &[&str], message: impl Into<String>) -> Diagnostic {
        Diagnostic::at(&self.file(), &self.html, needles, message)
    }

    pub(super) fn route_row(&self, pages: usize, kind: impl Into<BuildRouteKind>) -> BuildRouteRow {
        BuildRouteRow {
            route: self.source.route.as_str().to_string(),
            kind: kind.into(),
            pages,
        }
    }

    /// Whether a paginated/collection data source differs per locale.
    pub(super) fn collection_varies_by_locale(
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

    pub(super) fn shared_collection_items(
        &self,
        collection_id: &str,
        needle_refs: &[&str],
    ) -> Result<Vec<Value>> {
        let list = self
            .data
            .get(collection_id)
            .ok_or_else(|| self.at(needle_refs, missing_data_source_message(collection_id)))?;
        list.array().ok_or_else(|| {
            let value = list.value();
            self.at(
                needle_refs,
                format!("collection `{collection_id}` must be an array, got {value}"),
            )
        })
    }
}

/// Overlay locale catalog arrays onto page data (i18n-driven `data-each` sources).
fn merge_i18n_data(
    page_data: &HashMap<String, DataSource>,
    catalog: Option<&Value>,
) -> HashMap<String, DataSource> {
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
