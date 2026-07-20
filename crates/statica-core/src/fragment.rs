//! Fragment registry — templates discovered via `rel="statica/fragment"`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::funnel::{self, BindDecl, DataSource};
use crate::parse::{self, Document, Element, Node};

#[derive(Debug, Clone)]
pub struct Fragment {
    pub id: String,
    pub path: PathBuf,
    pub template: Element,
    /// Bind scope from `<template data-bind="name">` or `data-bind="{a, b}"`.
    pub bind: BindDecl,
    pub scope_id: String,
    pub data: HashMap<String, DataSource>,
}

pub struct FragmentRegistry {
    fragments: HashMap<String, Fragment>,
    data_cache: HashMap<PathBuf, serde_json::Value>,
}

impl FragmentRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            fragments: HashMap::new(),
            data_cache: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Fragment> {
        self.fragments.get(id)
    }

    pub fn data_cache_mut(&mut self) -> &mut HashMap<PathBuf, serde_json::Value> {
        &mut self.data_cache
    }

    pub fn load_links_from_document(&mut self, doc: &Document, base_dir: &Path) -> Result<()> {
        for (id, href) in funnel::find_fragment_links(doc) {
            self.ensure_loaded(&id, &href, base_dir)?;
        }
        Ok(())
    }

    pub fn ensure_loaded(&mut self, id: &str, href: &str, from_dir: &Path) -> Result<&Fragment> {
        if self.fragments.contains_key(id) {
            return self
                .fragments
                .get(id)
                .ok_or_else(|| Error::MissingFragment {
                    id: id.to_string(),
                });
        }

        let path = resolve_path(from_dir, href)?;
        let raw = fs::read_to_string(&path)
            .map_err(|e| Error::read(path.display().to_string(), e))?;
        let file_doc = parse::parse_fragment(&raw)?;
        let base_dir = path.parent().unwrap_or(from_dir);

        let data = funnel::load_data_from_document(&file_doc, base_dir, &mut self.data_cache)?;
        let nested = funnel::find_fragment_links(&file_doc);
        for (nid, nhref) in &nested {
            if !self.fragments.contains_key(nid) {
                self.ensure_loaded(nid, nhref, base_dir)?;
            }
        }

        let template_el = funnel::find_template(&file_doc, id).ok_or_else(|| {
            Error::MissingTemplate {
                id: id.to_string(),
                path: path.display().to_string(),
            }
        })?;
        let file = path.display().to_string();
        let bind_source = funnel::BindSource {
            file: &file,
            source: &raw,
        };
        let bind = match funnel::parse_bind_decl(template_el.attr("data-bind")) {
            Ok(decl) => decl,
            Err(reason) => {
                let prop = template_el.attr("data-bind").unwrap_or("").to_string();
                let dq = format!("data-bind=\"{prop}\"");
                let sq = format!("data-bind='{prop}'");
                let (file, line, column, snippet) =
                    crate::loc::locate_any(&file, &raw, &[&dq, &sq, prop.as_str()]);
                return Err(Error::InvalidBindProp {
                    file,
                    line,
                    column,
                    id: id.to_string(),
                    prop,
                    reason,
                    snippet,
                });
            }
        };
        funnel::validate_template_binds(id, &bind, &template_el.children, bind_source)?;
        let hash = short_hash(&raw);
        let scope_id = format!("{id}-{hash}");

        let frag = Fragment {
            id: id.to_string(),
            path,
            template: template_el.clone(),
            bind,
            scope_id,
            data,
        };
        self.fragments.insert(id.to_string(), frag);
        self.fragments
            .get(id)
            .ok_or_else(|| Error::MissingFragment {
                id: id.to_string(),
            })
    }
}

impl Default for FragmentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn short_hash(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(&hasher.finalize()[..4])
}

fn resolve_path(base_dir: &Path, rel: &str) -> Result<PathBuf> {
    let joined = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        base_dir.join(rel)
    };
    if let Ok(canon) = joined.canonicalize() {
        return Ok(canon);
    }
    if joined.exists() {
        return Ok(joined);
    }
    Err(Error::PathNotFound {
        path: joined.display().to_string(),
    })
}

/// Clone template element children as a mountable node list (without the `<template>` wrapper).
#[must_use]
pub fn template_children(frag: &Fragment) -> Vec<Node> {
    frag.template.children.clone()
}
