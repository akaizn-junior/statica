//! Fragment registry — templates discovered via `rel="statica/fragment"`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::aliases::{self, AliasOptions};
use crate::error::{Error, Result};
use crate::funnel::{self, BindDecl, DataSource};
use crate::parse::{self, Document, Element, Node};
use crate::render::RenderPlan;
use crate::scope;
use crate::tokens::DATA_BIND;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FragmentId(String);

impl FragmentId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

#[derive(Debug, Clone)]
pub struct FragmentFile(PathBuf);

impl FragmentFile {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct Fragment {
    pub id: String,
    pub path: FragmentFile,
    pub template: Element,
    pub render_plan: RenderPlan,
    /// Bind scope from `<template data-bind="name">` or `data-bind="{a, b}"`.
    pub bind: BindDecl,
    /// Static funnel sources loaded at registry time.
    pub data: HashMap<String, DataSource>,
    /// Whether the fragment declares dynamic funnel sources.
    pub has_dynamic_data: bool,
}

pub struct FragmentRegistry {
    site_root: PathBuf,
    fragments: HashMap<FragmentId, Fragment>,
    data_cache: HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
    extra_bind_roots: Vec<String>,
}

impl FragmentRegistry {
    #[must_use]
    pub fn new(site_root: impl Into<PathBuf>) -> Self {
        Self::with_extra_bind_roots(site_root, &[])
    }

    #[must_use]
    pub fn with_extra_bind_roots(
        site_root: impl Into<PathBuf>,
        extra_bind_roots: &[String],
    ) -> Self {
        Self {
            site_root: site_root.into(),
            fragments: HashMap::new(),
            data_cache: HashMap::new(),
            extra_bind_roots: extra_bind_roots.to_vec(),
        }
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Fragment> {
        self.fragments.get(&FragmentId::new(id))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.fragments.len()
    }

    pub fn data_cache_mut(
        &mut self,
    ) -> &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>> {
        &mut self.data_cache
    }

    pub fn load_links_from_document(
        &mut self,
        doc: &Document,
        base_dir: &Path,
        aliases: &AliasOptions,
        page: Option<(&str, &str)>,
    ) -> Result<()> {
        for (id, href) in funnel::find_fragment_links(doc) {
            self.ensure_loaded(&id, &href, base_dir, aliases, page)?;
        }
        Ok(())
    }

    pub fn ensure_loaded(
        &mut self,
        id: &str,
        href: &str,
        from_dir: &Path,
        aliases: &AliasOptions,
        page: Option<(&str, &str)>,
    ) -> Result<&Fragment> {
        let fragment_id = FragmentId::new(id);
        if self.fragments.contains_key(&fragment_id) {
            return self.fragments.get(&fragment_id).ok_or_else(|| {
                Error::at_file("<registry>", format!("missing fragment id `{id}`"))
            });
        }

        let href = aliases::resolve_path(href, aliases, page, "href")?;
        let path = resolve_local_path(&self.site_root, from_dir, &href, page, &href)?;
        let raw =
            fs::read_to_string(&path).map_err(|e| Error::read(path.display().to_string(), e))?;
        let file = path.display().to_string();
        let file_doc = parse::parse_fragment(&raw).map_err(|e| e.in_file(&file, &raw))?;
        let base_dir = path.parent().unwrap_or(from_dir);

        let data = funnel::load_data_from_document(
            &file_doc,
            &self.site_root,
            base_dir,
            &mut self.data_cache,
            aliases,
            Some((&file, &raw)),
        )?;
        let nested = funnel::find_fragment_links(&file_doc);
        for (nid, nhref) in &nested {
            if !self.fragments.contains_key(&FragmentId::new(nid)) {
                self.ensure_loaded(nid, nhref, base_dir, aliases, Some((&file, &raw)))?;
            }
        }

        let id_dq = format!("id=\"{id}\"");
        let id_sq = format!("id='{id}'");
        let template_el = funnel::find_template(&file_doc, id).ok_or_else(|| {
            Error::at(
                &file,
                &raw,
                &["<template", id_dq.as_str(), id_sq.as_str()],
                format!("fragment `{id}` has no matching <template id=\"{id}\">"),
            )
        })?;
        let bind_source = funnel::BindSource {
            file: &file,
            source: &raw,
        };
        let bind = match funnel::parse_bind_decl(template_el.bind_directive()) {
            Ok(decl) => decl,
            Err(reason) => {
                let prop = template_el.bind_directive().unwrap_or("").to_string();
                let dq = format!("{DATA_BIND}=\"{prop}\"");
                let sq = format!("{DATA_BIND}='{prop}'");
                return Err(Error::at(
                    &file,
                    &raw,
                    &[&dq, &sq, prop.as_str()],
                    format!("fragment `{id}` data-bind=`{prop}` is invalid — {reason}"),
                ));
            }
        };
        let mut bind_roots = funnel::data_link_ids(&file_doc);
        bind_roots.extend(self.extra_bind_roots.iter().cloned());
        funnel::validate_template_binds_with_roots(
            id,
            &bind,
            &template_el.children,
            bind_source,
            &bind_roots,
        )?;
        let hash = short_hash(&raw);
        let scope_id = format!("{id}-{hash}");
        let has_dynamic_data = funnel::document_has_dynamic_data(&file_doc);

        let mut template = template_el.clone();
        scope::apply_scope_to_nodes(&mut template.children, &scope_id);
        scope::rewrite_scripts_in_nodes(&mut template.children, &scope_id);

        let frag = Fragment {
            id: id.to_string(),
            path: FragmentFile::new(path),
            render_plan: RenderPlan::compile_fragment(&template.children),
            template,
            bind,
            data,
            has_dynamic_data,
        };
        self.fragments.insert(fragment_id.clone(), frag);
        self.fragments
            .get(&fragment_id)
            .ok_or_else(|| Error::at_file("<registry>", format!("missing fragment id `{id}`")))
    }

    /// Merge static fragment funnel data with dynamic sources when the parent page locale is known.
    pub fn resolve_fragment_data(
        &self,
        frag: &Fragment,
        current: Option<&serde_json::Value>,
        locale: Option<&str>,
        data_cache: &mut HashMap<PathBuf, std::sync::Arc<crate::content::DataSet>>,
        aliases: &AliasOptions,
    ) -> Result<HashMap<String, DataSource>> {
        let mut data = frag.data.clone();
        if !frag.has_dynamic_data {
            return Ok(data);
        }
        let raw = fs::read_to_string(frag.path.as_path())
            .map_err(|e| Error::read(frag.path.as_path().display().to_string(), e))?;
        let file = frag.path.as_path().display().to_string();
        let file_doc = parse::parse_fragment(&raw).map_err(|e| e.in_file(&file, &raw))?;
        let base_dir = frag
            .path
            .as_path()
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let mut template_context =
            match funnel::bind_context(&frag.bind, current.unwrap_or(&serde_json::Value::Null)) {
                serde_json::Value::Object(map) => map,
                _ => serde_json::Map::new(),
            };
        let mut document_context = serde_json::Map::new();
        if let Some(loc) = locale {
            let i18n = serde_json::json!({ "locale": loc });
            document_context.insert("i18n".into(), i18n.clone());
            template_context.insert("i18n".into(), i18n);
        }
        let locale_data = funnel::load_dynamic_data_from_fragment_template(
            &file_doc,
            &frag.id,
            &self.site_root,
            base_dir,
            data_cache,
            aliases,
            &serde_json::Value::Object(document_context),
            &serde_json::Value::Object(template_context),
            Some((&file, &raw)),
        )?;
        for (id, source) in locale_data {
            data.insert(id, source);
        }
        Ok(data)
    }
}

impl Default for FragmentRegistry {
    fn default() -> Self {
        Self::new(".")
    }
}

fn short_hash(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(&hasher.finalize()[..4])
}

fn resolve_local_path(
    site_root: &Path,
    page_dir: &Path,
    rel: &str,
    page: Option<(&str, &str)>,
    href: &str,
) -> Result<PathBuf> {
    let joined = aliases::resolve_local_href(site_root, page_dir, rel);
    if let Ok(canon) = joined.canonicalize() {
        return Ok(canon);
    }
    if joined.exists() {
        return Ok(joined);
    }
    let path = joined.display().to_string();
    if let Some((file, source)) = page {
        let dq = format!("href=\"{href}\"");
        let sq = format!("href='{href}'");
        return Err(Error::at(
            file,
            source,
            &[&dq, &sq, href],
            format!("path not found: {path}"),
        ));
    }
    Err(Error::at_file(
        path.clone(),
        format!("path not found: {path}"),
    ))
}

/// Clone template element children as a mountable node list (without the `<template>` wrapper).
#[must_use]
pub fn template_children(frag: &Fragment) -> Vec<Node> {
    frag.template.children.clone()
}
