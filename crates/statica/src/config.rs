//! Project configuration (`statica.toml`) — owned by the CLI.
//!
//! # Responsibilities
//!
//! - Load optional `statica.toml` from a config directory (defaults if absent).
//! - Map user settings into [`statica_core::BuildOptions`].
//! - Apply CLI SPEC overrides via [`StaticaConfig::apply_cli`].
//!
//! Core never reads config files; only the CLI does.
//!
//! # Project resolution
//!
//! See [`crate::cmd::util::load_project`]: cwd → walk up for this file → optional
//! [`StaticaConfig::project`] subdirectory.

use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use statica_core::{
    AliasOptions, AssetProcessOptions, BuildOptions, EmitOptions, FormsOptions, I18nOptions,
    PaginationRule, RssOptions, SitemapOptions,
};

/// Canonical config file name in a statica project root.
pub const CONFIG_FILE: &str = "statica.toml";

/// End-user project settings. Missing file → [`StaticaConfig::default`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StaticaConfig {
    /// Site root relative to this config file (empty = this directory).
    ///
    /// Monorepo: keep `statica.toml` at the repo root and set e.g. `project = "apps/docs"`.
    #[serde(default)]
    pub project: String,
    pub out_dir: String,
    pub clean: bool,
    pub copy_assets: bool,
    pub asset_dirs: Vec<String>,
    pub ignore_dirs: Vec<String>,
    /// Absolute site origin for sitemap/RSS (`https://example.com`). Empty → feeds skipped.
    pub site_url: String,
    pub emit: EmitConfig,
    pub process: ProcessConfig,
    pub sitemap: SitemapConfig,
    pub rss: RssConfig,
    /// Paginated listings (`[[pagination]]`).
    pub pagination: Vec<PaginationConfig>,
    /// Preview / watch HTTP server (`serve` + `watch`).
    #[serde(alias = "watch")]
    pub preview: PreviewConfig,
    /// Path / URL aliases for authoring (`@Name:tail` in hrefs).
    pub aliases: AliasesConfig,
    /// Static form wiring (`[forms]`).
    pub forms: FormsConfig,
    /// Build-time env vars and optional `.env` / `.dev.vars` loading.
    pub env: crate::env::EnvConfig,
    /// Locale catalogs and prefixed routes (`[i18n]`).
    pub i18n: I18nConfig,
}

/// `[aliases]` — `@Name:tail` → resolved URL or path (see `docs/guide.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AliasesConfig {
    /// Leading symbol for alias references (default `@`).
    pub symbol: String,
    /// Alias name → base URL or local path prefix.
    pub paths: std::collections::HashMap<String, String>,
}

impl Default for AliasesConfig {
    fn default() -> Self {
        AliasOptions::default().into()
    }
}

impl From<AliasOptions> for AliasesConfig {
    fn from(opts: AliasOptions) -> Self {
        Self {
            symbol: opts.symbol,
            paths: opts.paths,
        }
    }
}

impl AliasesConfig {
    #[must_use]
    pub fn to_core(&self) -> AliasOptions {
        AliasOptions {
            symbol: self.symbol.clone(),
            paths: self.paths.clone(),
        }
    }
}

/// `[forms]` — wire `<form statica>` to a provider endpoint at build time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FormsConfig {
    pub enabled: bool,
    /// `formspree` (default) or `custom`.
    pub provider: String,
    /// Formspree: URL template with `{id}`. Custom: single POST URL.
    pub endpoint: String,
    /// Logical form name → provider form id.
    #[serde(default)]
    pub ids: HashMap<String, String>,
    /// Env var name that overrides `endpoint` when set (default `FORMS_ENDPOINT`).
    pub endpoint_env: String,
}

impl Default for FormsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "formspree".into(),
            endpoint: "https://formspree.io/f/{id}".into(),
            ids: HashMap::new(),
            endpoint_env: "FORMS_ENDPOINT".into(),
        }
    }
}

impl FormsConfig {
    /// Apply build-time env overrides (`FORMS_ENDPOINT`, `FORMS_{NAME}_ID`).
    pub fn resolve_env(&mut self) {
        if let Ok(v) = std::env::var(&self.endpoint_env) {
            if !v.is_empty() {
                self.endpoint = v;
            }
        }
        for (key, id) in &mut self.ids {
            let env_key = format!(
                "FORMS_{}_ID",
                key.to_ascii_uppercase().replace('-', "_")
            );
            if let Ok(v) = std::env::var(&env_key) {
                if !v.is_empty() {
                    *id = v;
                }
            }
        }
    }

    #[must_use]
    pub fn to_core(&self) -> FormsOptions {
        FormsOptions {
            enabled: self.enabled,
            provider: FormsOptions::provider_from_str(&self.provider),
            endpoint: self.endpoint.clone(),
            ids: self.ids.clone(),
        }
    }
}

/// `[emit]` — what to strip / tidy in written HTML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EmitConfig {
    /// Remove `<script type="statica/data">` from output.
    pub strip_data: bool,
    /// Remove `<link rel="statica/fragment">` from output.
    pub strip_fragments: bool,
    /// Remove `data-bind` from `<html>`.
    pub strip_html_data_bind: bool,
    /// Dedupe inlined `$` helpers.
    pub dedupe_helpers: bool,
    /// Dedupe scoped `<style>` blocks.
    pub dedupe_styles: bool,
}

/// `[process]` — asset optimize pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessConfig {
    pub enabled: bool,
    pub css: bool, // lightningcss: modern → browser-ready + minify for asset .css
    pub js: bool,
    pub images: bool,
    pub fonts: bool,
}

/// `[sitemap]` — XML sitemap of all emitted pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SitemapConfig {
    pub enabled: bool,
    pub filename: String,
    /// URLs per file before splitting into parts + a sitemap index (max 50_000).
    pub urls_per_file: usize,
}

/// `[[pagination]]` — chunk a data array into `route/[page]/` pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaginationConfig {
    /// Page template route, e.g. `blog/[page]` or `posts/[page]`.
    pub route: String,
    /// Items per generated page.
    #[serde(alias = "per_page")]
    pub page_size: usize,
    /// Max items after `offset` (0 = unlimited).
    #[serde(default)]
    pub limit: usize,
    /// Skip this many items before limit / paging.
    #[serde(default)]
    pub offset: usize,
    /// Sort field before slicing (empty = JSON order).
    #[serde(default)]
    pub sort_by: String,
    /// Sort descending when `sort_by` is set.
    #[serde(default)]
    pub sort_desc: bool,
    /// Cap emitted page folders (0 = unlimited).
    #[serde(default)]
    pub max_pages: usize,
    /// Also emit page 1 at the parent path (`blog/` for `blog/[page]`).
    #[serde(default)]
    pub index: bool,
}

/// `[rss]` — RSS 2.0 feed from collection pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RssConfig {
    pub enabled: bool,
    pub filename: String,
    pub title: String,
    pub description: String,
    pub language: String,
    /// Max items (0 = unlimited).
    pub limit: usize,
    pub title_field: String,
    pub description_field: String,
    pub date_field: String,
    /// Data ids to include; empty = every collection.
    pub collections: Vec<String>,
}

/// `[preview]` (alias `[watch]`) — local HTTP preview for `serve` / `watch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewConfig {
    /// Bind address (default `0.0.0.0` so phones on Wi‑Fi can connect).
    pub host: String,
    pub port: u16,
    pub debounce_ms: u64,
    pub poll_interval_secs: u64,
}

/// `[i18n]` — `[locale]/…` route expansion + `data-t` translation catalogs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct I18nConfig {
    /// Enable translation catalogs and `data-t` binding.
    pub enabled: bool,
    /// Default locale for routes without a locale prefix (must appear in `locales`).
    #[serde(default = "default_i18n_locale")]
    pub default: String,
    /// Locale codes expanded for `[locale]/…` page templates.
    pub locales: Vec<String>,
    /// Catalog directory under the site root: `{dir}/{locale}.json`.
    #[serde(default = "default_i18n_dir")]
    pub dir: String,
    /// Fallback catalog for missing keys (empty → `default`).
    #[serde(default)]
    pub fallback: String,
}

fn default_i18n_locale() -> String {
    "en".into()
}

fn default_i18n_dir() -> String {
    "content/i18n".into()
}

impl Default for I18nConfig {
    fn default() -> Self {
        I18nOptions::default().into()
    }
}

impl From<I18nOptions> for I18nConfig {
    fn from(opts: I18nOptions) -> Self {
        Self {
            enabled: opts.enabled,
            default: opts.default_locale,
            locales: opts.locales,
            dir: opts.dir,
            fallback: opts.fallback,
        }
    }
}

impl I18nConfig {
    #[must_use]
    pub fn to_core(&self) -> I18nOptions {
        I18nOptions {
            enabled: self.enabled,
            default_locale: self.default.clone(),
            locales: if self.locales.is_empty() {
                vec![self.default.clone()]
            } else {
                self.locales.clone()
            },
            dir: self.dir.clone(),
            fallback: self.fallback.clone(),
        }
    }
}

impl Default for StaticaConfig {
    fn default() -> Self {
        Self {
            project: String::new(),
            out_dir: ".dist".into(),
            clean: true,
            copy_assets: true,
            asset_dirs: vec!["public".into(), "assets".into(), "static".into()],
            ignore_dirs: vec![
                ".dist".into(),
                "dist".into(),
                "target".into(),
                ".git".into(),
            ],
            site_url: String::new(),
            emit: EmitConfig::default(),
            process: ProcessConfig::default(),
            sitemap: SitemapConfig::default(),
            rss: RssConfig::default(),
            pagination: Vec::new(),
            preview: PreviewConfig::default(),
            aliases: AliasesConfig::default(),
            forms: FormsConfig::default(),
            env: crate::env::EnvConfig::default(),
            i18n: I18nConfig::default(),
        }
    }
}

impl Default for EmitConfig {
    fn default() -> Self {
        Self {
            strip_data: true,
            strip_fragments: true,
            strip_html_data_bind: true,
            dedupe_helpers: true,
            dedupe_styles: true,
        }
    }
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            css: true,
            js: true,
            images: true,
            fonts: false,
        }
    }
}

impl Default for SitemapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            filename: "sitemap.xml".into(),
            urls_per_file: 50_000,
        }
    }
}

impl Default for RssConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            filename: "rss.xml".into(),
            title: String::new(),
            description: String::new(),
            language: "en".into(),
            limit: 50,
            title_field: "headline".into(),
            description_field: "summary".into(),
            date_field: "published_at".into(),
            collections: Vec::new(),
        }
    }
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 4321,
            debounce_ms: 80,
            poll_interval_secs: 2,
        }
    }
}

impl EmitConfig {
    #[must_use]
    pub fn to_core(&self) -> EmitOptions {
        EmitOptions {
            strip_data: self.strip_data,
            strip_fragments: self.strip_fragments,
            strip_html_data_bind: self.strip_html_data_bind,
            dedupe_helpers: self.dedupe_helpers,
            dedupe_styles: self.dedupe_styles,
        }
    }
}

impl ProcessConfig {
    #[must_use]
    pub fn to_core(&self) -> AssetProcessOptions {
        AssetProcessOptions {
            enabled: self.enabled,
            css: self.css,
            js: self.js,
            images: self.images,
            fonts: self.fonts,
        }
    }
}

impl SitemapConfig {
    #[must_use]
    pub fn to_core(&self) -> SitemapOptions {
        SitemapOptions {
            enabled: self.enabled,
            filename: self.filename.clone(),
            urls_per_file: self.urls_per_file,
        }
    }
}

impl PaginationConfig {
    #[must_use]
    pub fn to_core(&self) -> PaginationRule {
        PaginationRule {
            route: self.route.clone(),
            page_size: self.page_size.max(1),
            limit: self.limit,
            offset: self.offset,
            sort_by: self.sort_by.clone(),
            sort_desc: self.sort_desc,
            max_pages: self.max_pages,
            index: self.index,
        }
    }
}

impl RssConfig {
    #[must_use]
    pub fn to_core(&self) -> RssOptions {
        RssOptions {
            enabled: self.enabled,
            filename: self.filename.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            language: self.language.clone(),
            limit: self.limit,
            title_field: self.title_field.clone(),
            description_field: self.description_field.clone(),
            date_field: self.date_field.clone(),
            collections: self.collections.clone(),
        }
    }
}

impl PreviewConfig {
    /// Parse bind host (IPv4/IPv6).
    pub fn host_addr(&self) -> Result<IpAddr> {
        IpAddr::from_str(&self.host)
            .with_context(|| format!("invalid [preview].host `{}`", self.host))
    }
}

impl StaticaConfig {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(CONFIG_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("invalid {} ({})", CONFIG_FILE, path.display()))
    }

    /// Apply `[env]` files/vars, then resolve form env placeholders.
    pub fn apply_env(&mut self, config_dir: &Path) -> Result<()> {
        crate::env::apply(config_dir, &self.env)?;
        self.forms.resolve_env();
        Ok(())
    }

    #[must_use]
    pub fn out_dir_path(&self, root: &Path) -> PathBuf {
        if Path::new(&self.out_dir).is_absolute() {
            PathBuf::from(&self.out_dir)
        } else {
            root.join(&self.out_dir)
        }
    }

    #[must_use]
    pub fn to_build_options(&self, root: impl Into<PathBuf>) -> BuildOptions {
        let root = root.into();
        let mut ignore_dirs = self.ignore_dirs.clone();
        let out_name = Path::new(&self.out_dir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(self.out_dir.as_str());
        if !ignore_dirs.iter().any(|d| d == out_name) {
            ignore_dirs.push(out_name.to_string());
        }
        BuildOptions {
            out_dir: self.out_dir_path(&root),
            copy_assets: self.copy_assets,
            site_url: self.site_url.clone(),
            sitemap: self.sitemap.to_core(),
            rss: self.rss.to_core(),
            pagination: self.pagination.iter().map(PaginationConfig::to_core).collect(),
            process: self.process.to_core(),
            emit: self.emit.to_core(),
            aliases: self.aliases.to_core(),
            forms: self.forms.to_core(),
            i18n: self.i18n.to_core(),
            clean: self.clean,
            asset_dirs: self.asset_dirs.clone(),
            ignore_dirs,
            root,
            verbose: false,
        }
    }

    /// Apply CLI flag / SPEC overrides. CLI wins over values loaded from disk.
    ///
    /// Nested tables are applied from compact SPECs (`--rss 'title=…,limit=20'`,
    /// `--pagination 'route=…'`, etc.). See `docs/guide.md`.
    pub fn apply_cli(&mut self, cli: &crate::cli::ConfigCli) -> Result<()> {
        if let Some(v) = &cli.project {
            self.project = v.clone();
        }
        if let Some(v) = &cli.out_dir {
            self.out_dir = v.clone();
        }
        if let Some(v) = &cli.site_url {
            self.site_url = v.clone();
        }
        apply_bool(cli.clean, cli.no_clean, &mut self.clean);
        apply_bool(cli.copy_assets, cli.no_copy_assets, &mut self.copy_assets);
        if let Some(v) = &cli.asset_dirs {
            self.asset_dirs = v.clone();
        }
        if let Some(v) = &cli.ignore_dirs {
            self.ignore_dirs = v.clone();
        }

        if let Some(spec) = &cli.emit {
            apply_emit_spec(&mut self.emit, spec)?;
        }

        if cli.no_process {
            self.process.enabled = false;
        } else if let Some(spec) = &cli.process {
            self.process.enabled = true;
            if !spec.is_empty() {
                apply_process_spec(&mut self.process, spec)?;
            }
        }

        if cli.no_sitemap {
            self.sitemap.enabled = false;
        } else if let Some(spec) = &cli.sitemap {
            self.sitemap.enabled = true;
            if !spec.is_empty() {
                apply_sitemap_spec(&mut self.sitemap, spec)?;
            }
        }

        if cli.no_rss {
            self.rss.enabled = false;
        } else if let Some(spec) = &cli.rss {
            self.rss.enabled = true;
            if !spec.is_empty() {
                apply_rss_spec(&mut self.rss, spec)?;
            }
        }

        if !cli.pagination.is_empty() {
            self.pagination = cli
                .pagination
                .iter()
                .map(|s| parse_pagination_spec(s))
                .collect::<Result<Vec<_>>>()?;
        }

        if let Some(spec) = &cli.preview {
            apply_preview_spec(&mut self.preview, spec)?;
        }
        if let Some(v) = &cli.host {
            self.preview.host = v.clone();
        }
        if let Some(v) = cli.port {
            self.preview.port = v;
        }

        if cli.no_i18n {
            self.i18n.enabled = false;
        } else if let Some(spec) = &cli.i18n {
            self.i18n.enabled = true;
            if !spec.is_empty() {
                apply_i18n_spec(&mut self.i18n, spec)?;
            }
        }

        Ok(())
    }

    #[must_use]
    pub fn default_toml() -> String {
        r#"# statica.toml — all keys optional; defaults shown

# Site root relative to this file (empty = this directory).
# Monorepo example: keep statica.toml at the repo root and set:
# project = "apps/docs"
project = ""

out_dir = ".dist"
clean = true
copy_assets = true
asset_dirs = ["public", "assets", "static"]
ignore_dirs = [".dist", "dist", "target", ".git"]
site_url = ""                  # e.g. "https://example.com" — needed for sitemap/RSS

# Authoring aliases — @Name:tail in hrefs (symbol defaults to @)
[aliases]
symbol = "@"

[aliases.paths]
Google = "https://fonts.googleapis.com/css2"
# fonts = "./assets/fonts"     # local: @fonts:outfit.css → ./assets/fonts/outfit.css

# HTML emit: strip authoring tags from .dist
[emit]
strip_data = true              # <script type="statica/data">
strip_fragments = true         # <link rel="statica/fragment">
strip_html_data_bind = true    # data-bind on <html>
dedupe_helpers = true
dedupe_styles = true

# Asset optimize (also: statica --process)
[process]
enabled = false
css = true
js = true
images = true
fonts = false

# XML sitemap of every emitted page (needs site_url)
[sitemap]
enabled = false
filename = "sitemap.xml"
urls_per_file = 50000          # over this → sitemap-1.xml… + index at filename

# RSS 2.0 from collection pages (needs site_url)
[rss]
enabled = false
filename = "rss.xml"
title = ""
description = ""
language = "en"
limit = 50
title_field = "headline"
description_field = "summary"
date_field = "published_at"
collections = []               # empty = all collections; or ["posts"]

# Paginated listings — template at route with [page], data via <html data-bind>
# [[pagination]]
# route = "blog/[page]"        # → .dist/blog/1/, blog/2/, …
# page_size = 10               # alias: per_page
# limit = 0                    # max items after offset (0 = all)
# offset = 0                   # skip first N items
# sort_by = "published_at"     # empty = keep JSON order
# sort_desc = true
# max_pages = 0                # cap page folders (0 = all)
# index = true                 # also write page 1 at blog/

# Local preview for `statica serve` / `statica watch`
[preview]
host = "0.0.0.0"
port = 4321
debounce_ms = 80
poll_interval_secs = 2

# Internationalization — locale from folder structure + content/i18n/{locale}.json
# [i18n]
# enabled = false
# default = "en"
# locales = ["en", "pt"]     # expanded for every [locale]/… template
# dir = "content/i18n"
# fallback = ""             # empty → default locale catalog
"#
        .into()
    }
}

fn apply_bool(set: bool, unset: bool, target: &mut bool) {
    if set {
        *target = true;
    } else if unset {
        *target = false;
    }
}

fn parse_bool(raw: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => anyhow::bail!("invalid bool `{other}` (use true/false)"),
    }
}

fn for_each_kv(spec: &str, mut f: impl FnMut(&str, &str) -> Result<()>) -> Result<()> {
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, value) = part
            .split_once('=')
            .with_context(|| format!("SPEC needs key=value, got `{part}`"))?;
        f(key.trim(), value.trim())?;
    }
    Ok(())
}

fn apply_emit_spec(cfg: &mut EmitConfig, spec: &str) -> Result<()> {
    for_each_kv(spec, |key, value| {
        match key {
            "strip_data" => cfg.strip_data = parse_bool(value)?,
            "strip_fragments" => cfg.strip_fragments = parse_bool(value)?,
            "strip_html_data_bind" => cfg.strip_html_data_bind = parse_bool(value)?,
            "dedupe_helpers" => cfg.dedupe_helpers = parse_bool(value)?,
            "dedupe_styles" => cfg.dedupe_styles = parse_bool(value)?,
            other => anyhow::bail!("unknown emit key `{other}`"),
        }
        Ok(())
    })
}

fn apply_process_spec(cfg: &mut ProcessConfig, spec: &str) -> Result<()> {
    for_each_kv(spec, |key, value| {
        match key {
            "enabled" => cfg.enabled = parse_bool(value)?,
            "css" => cfg.css = parse_bool(value)?,
            "js" => cfg.js = parse_bool(value)?,
            "images" => cfg.images = parse_bool(value)?,
            "fonts" => cfg.fonts = parse_bool(value)?,
            other => anyhow::bail!("unknown process key `{other}`"),
        }
        Ok(())
    })
}

fn apply_sitemap_spec(cfg: &mut SitemapConfig, spec: &str) -> Result<()> {
    for_each_kv(spec, |key, value| {
        match key {
            "enabled" => cfg.enabled = parse_bool(value)?,
            "filename" => cfg.filename = value.to_string(),
            "urls_per_file" => {
                cfg.urls_per_file = value
                    .parse()
                    .with_context(|| format!("invalid urls_per_file `{value}`"))?;
            }
            other => anyhow::bail!("unknown sitemap key `{other}`"),
        }
        Ok(())
    })
}

fn apply_rss_spec(cfg: &mut RssConfig, spec: &str) -> Result<()> {
    for_each_kv(spec, |key, value| {
        match key {
            "enabled" => cfg.enabled = parse_bool(value)?,
            "filename" => cfg.filename = value.to_string(),
            "title" => cfg.title = value.to_string(),
            "description" => cfg.description = value.to_string(),
            "language" => cfg.language = value.to_string(),
            "limit" => {
                cfg.limit = value
                    .parse()
                    .with_context(|| format!("invalid limit `{value}`"))?;
            }
            "title_field" => cfg.title_field = value.to_string(),
            "description_field" => cfg.description_field = value.to_string(),
            "date_field" => cfg.date_field = value.to_string(),
            "collections" => {
                cfg.collections = value
                    .split('|')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            other => anyhow::bail!("unknown rss key `{other}`"),
        }
        Ok(())
    })
}

fn apply_preview_spec(cfg: &mut PreviewConfig, spec: &str) -> Result<()> {
    for_each_kv(spec, |key, value| {
        match key {
            "host" => cfg.host = value.to_string(),
            "port" => {
                cfg.port = value
                    .parse()
                    .with_context(|| format!("invalid port `{value}`"))?;
            }
            "debounce_ms" => {
                cfg.debounce_ms = value
                    .parse()
                    .with_context(|| format!("invalid debounce_ms `{value}`"))?;
            }
            "poll_interval_secs" => {
                cfg.poll_interval_secs = value
                    .parse()
                    .with_context(|| format!("invalid poll_interval_secs `{value}`"))?;
            }
            other => anyhow::bail!("unknown preview key `{other}`"),
        }
        Ok(())
    })
}

fn parse_pagination_spec(spec: &str) -> Result<PaginationConfig> {
    let mut cfg = PaginationConfig {
        route: String::new(),
        page_size: 10,
        limit: 0,
        offset: 0,
        sort_by: String::new(),
        sort_desc: false,
        max_pages: 0,
        index: false,
    };
    for_each_kv(spec, |key, value| {
        match key {
            "route" => cfg.route = value.to_string(),
            "page_size" | "per_page" => {
                cfg.page_size = value
                    .parse()
                    .with_context(|| format!("invalid page_size `{value}`"))?;
            }
            "limit" => {
                cfg.limit = value
                    .parse()
                    .with_context(|| format!("invalid limit `{value}`"))?;
            }
            "offset" => {
                cfg.offset = value
                    .parse()
                    .with_context(|| format!("invalid offset `{value}`"))?;
            }
            "sort_by" => cfg.sort_by = value.to_string(),
            "sort_desc" => cfg.sort_desc = parse_bool(value)?,
            "max_pages" => {
                cfg.max_pages = value
                    .parse()
                    .with_context(|| format!("invalid max_pages `{value}`"))?;
            }
            "index" => cfg.index = parse_bool(value)?,
            other => anyhow::bail!("unknown pagination key `{other}`"),
        }
        Ok(())
    })?;
    if cfg.route.is_empty() {
        anyhow::bail!("pagination SPEC requires route=…");
    }
    Ok(cfg)
}

fn apply_i18n_spec(cfg: &mut I18nConfig, spec: &str) -> Result<()> {
    for_each_kv(spec, |key, value| {
        match key {
            "enabled" => cfg.enabled = parse_bool(value)?,
            "default" => cfg.default = value.to_string(),
            "locales" => {
                cfg.locales = value
                    .split('|')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "dir" => cfg.dir = value.to_string(),
            "fallback" => cfg.fallback = value.to_string(),
            other => anyhow::bail!("unknown i18n key `{other}`"),
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("statica-cli-config-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_without_file() {
        let dir = temp_dir();
        let cfg = StaticaConfig::load(&dir).unwrap();
        assert!(cfg.emit.strip_data);
        assert_eq!(cfg.preview.host, "0.0.0.0");
        assert_eq!(cfg.preview.port, 4321);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn loads_emit_and_preview() {
        let dir = temp_dir();
        fs::write(
            dir.join(CONFIG_FILE),
            r#"
[emit]
strip_data = false
strip_fragments = true

[preview]
host = "0.0.0.0"
port = 8080
"#,
        )
        .unwrap();
        let cfg = StaticaConfig::load(&dir).unwrap();
        assert!(!cfg.emit.strip_data);
        assert!(cfg.emit.strip_fragments);
        assert_eq!(cfg.preview.host, "0.0.0.0");
        assert_eq!(cfg.preview.port, 8080);
        assert_eq!(cfg.preview.host_addr().unwrap(), IpAddr::from_str("0.0.0.0").unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn watch_alias_still_loads() {
        let dir = temp_dir();
        fs::write(
            dir.join(CONFIG_FILE),
            r#"
[watch]
port = 9000
"#,
        )
        .unwrap();
        let cfg = StaticaConfig::load(&dir).unwrap();
        assert_eq!(cfg.preview.port, 9000);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn loads_forms() {
        let dir = temp_dir();
        fs::write(
            dir.join(CONFIG_FILE),
            r#"
[forms]
enabled = true
provider = "formspree"

[forms.ids]
contact = "xyzabc"
"#,
        )
        .unwrap();
        let mut cfg = StaticaConfig::load(&dir).unwrap();
        cfg.apply_env(&dir).unwrap();
        assert!(cfg.forms.enabled);
        assert_eq!(cfg.forms.provider, "formspree");
        assert_eq!(cfg.forms.ids.get("contact").map(String::as_str), Some("xyzabc"));
        let core = cfg.forms.to_core();
        assert!(core.enabled);
        assert_eq!(core.ids.get("contact").map(String::as_str), Some("xyzabc"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn env_files_override_forms_ids() {
        let dir = temp_dir();
        fs::write(
            dir.join(CONFIG_FILE),
            r#"
[forms]
enabled = true

[forms.ids]
contact = "from-config"
"#,
        )
        .unwrap();
        fs::write(
            dir.join(".env"),
            "FORMS_CONTACT_ID=from-dotenv\n",
        )
        .unwrap();
        fs::write(
            dir.join(".dev.vars"),
            "FORMS_CONTACT_ID=from-devvars\n",
        )
        .unwrap();

        let mut cfg = StaticaConfig::load(&dir).unwrap();
        cfg.apply_env(&dir).unwrap();
        assert_eq!(
            cfg.forms.ids.get("contact").map(String::as_str),
            Some("from-devvars")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn loads_aliases() {
        let dir = temp_dir();
        fs::write(
            dir.join(CONFIG_FILE),
            r#"
[aliases]
symbol = "@"

[aliases.paths]
Google = "https://fonts.googleapis.com/css2"
fonts = "./assets/fonts"
"#,
        )
        .unwrap();
        let cfg = StaticaConfig::load(&dir).unwrap();
        assert_eq!(cfg.aliases.symbol, "@");
        assert_eq!(
            cfg.aliases.paths.get("Google").map(String::as_str),
            Some("https://fonts.googleapis.com/css2")
        );
        assert_eq!(
            cfg.aliases.paths.get("fonts").map(String::as_str),
            Some("./assets/fonts")
        );
        let opts = cfg.to_build_options(&dir);
        let resolved = opts
            .aliases
            .parse("@Google/?family=Outfit&display=swap")
            .unwrap();
        assert_eq!(
            statica_core::join_alias(resolved.base, resolved.tail),
            "https://fonts.googleapis.com/css2?family=Outfit&display=swap"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn default_toml_roundtrips() {
        let cfg: StaticaConfig = toml::from_str(&StaticaConfig::default_toml()).unwrap();
        assert!(cfg.emit.strip_data);
        assert!(!cfg.sitemap.enabled);
        assert!(!cfg.rss.enabled);
        assert_eq!(cfg.preview.host, "0.0.0.0");
    }

    #[test]
    fn maps_feeds() {
        let mut cfg = StaticaConfig::default();
        cfg.site_url = "https://example.com".into();
        cfg.rss.enabled = true;
        cfg.rss.title = "Blog".into();
        let opts = cfg.to_build_options(PathBuf::from("/tmp/site"));
        assert_eq!(opts.site_url, "https://example.com");
        assert!(!opts.sitemap.enabled);
        assert!(opts.rss.enabled);
        assert_eq!(opts.rss.title, "Blog");
    }

    #[test]
    fn apply_cli_i18n() {
        let mut cfg = StaticaConfig::default();
        let cli = crate::cli::ConfigCli {
            i18n: Some("locales=en|pt,default=en".into()),
            ..crate::cli::ConfigCli::default()
        };
        cfg.apply_cli(&cli).unwrap();
        assert!(cfg.i18n.enabled);
        assert_eq!(cfg.i18n.locales, vec!["en", "pt"]);
        assert_eq!(cfg.i18n.default, "en");
    }

    #[test]
    fn loads_i18n_config() {
        let dir = temp_dir();
        fs::write(
            dir.join(CONFIG_FILE),
            r#"
[i18n]
enabled = true
default = "en"
locales = ["en", "pt"]
dir = "locales"
"#,
        )
        .unwrap();
        let cfg = StaticaConfig::load(&dir).unwrap();
        assert!(cfg.i18n.enabled);
        assert_eq!(cfg.i18n.locales, vec!["en", "pt"]);
        let core = cfg.i18n.to_core();
        assert_eq!(core.dir, "locales");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_cli_overrides() {
        let mut cfg = StaticaConfig::default();
        let cli = crate::cli::ConfigCli {
            process: Some(String::new()),
            no_sitemap: true,
            rss: Some("title=T,limit=3,collections=posts|notes".into()),
            site_url: Some("https://ex.com".into()),
            pagination: vec![
                "route=blog/[page],page_size=2,sort_desc=true,index=true".into(),
            ],
            emit: Some("strip_data=false".into()),
            preview: Some("port=9000,debounce_ms=50".into()),
            ..crate::cli::ConfigCli::default()
        };
        cfg.apply_cli(&cli).unwrap();
        assert!(cfg.process.enabled);
        assert!(!cfg.sitemap.enabled);
        assert!(cfg.rss.enabled);
        assert_eq!(cfg.rss.title, "T");
        assert_eq!(cfg.rss.limit, 3);
        assert_eq!(cfg.rss.collections, vec!["posts", "notes"]);
        assert!(!cfg.emit.strip_data);
        assert_eq!(cfg.preview.port, 9000);
        assert_eq!(cfg.site_url, "https://ex.com");
        assert_eq!(cfg.pagination.len(), 1);
        assert_eq!(cfg.pagination[0].page_size, 2);
        assert!(cfg.pagination[0].index);
    }
}
