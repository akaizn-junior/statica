//! Public build configuration, reporting, and render mode types.

use std::path::PathBuf;

use serde::Serialize;

use crate::aliases::AliasOptions;
use crate::assets::AssetProcessOptions;
use crate::feeds::{RssOptions, SitemapOptions};
use crate::forms::FormsOptions;
use crate::i18n::I18nOptions;
use crate::loc::Diagnostic;
use crate::minify::MinifyOptions;
use crate::paginate::PaginationRule;
use crate::search::SearchOptions;

/// Inputs for a build. The CLI maps `statica.toml` into this; core does not read config files.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub root: PathBuf,
    pub out_dir: PathBuf,
    pub copy_assets: bool,
    /// Absolute site origin for sitemap/RSS (e.g. `https://example.com`). Empty -> feeds skipped.
    pub site_url: String,
    pub sitemap: SitemapOptions,
    pub rss: RssOptions,
    /// Scaffold `public/manifest.webmanifest` and inject PWA head tags.
    pub manifest: bool,
    /// List -> `.../1/`, `.../2/`, ... expansions.
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
    /// Browser-side site search index and generated search controls.
    pub search: SearchOptions,
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
            search: SearchOptions::default(),
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

    pub(super) fn should_render_parallel(self) -> bool {
        matches!(self, Self::Auto | Self::Parallel)
    }
}

pub(super) fn render_detail(mode: RenderMode, threads: usize) -> String {
    match (mode, threads) {
        (RenderMode::Serial, _) => "serial".to_string(),
        (RenderMode::Auto, 0) => "auto".to_string(),
        (RenderMode::Parallel, 0) => "parallel auto-threads".to_string(),
        (RenderMode::Auto, n) => format!("auto, {n} threads"),
        (RenderMode::Parallel, n) => format!("parallel, {n} threads"),
    }
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

impl From<crate::discover::PageKind> for BuildRouteKind {
    fn from(kind: crate::discover::PageKind) -> Self {
        match kind {
            crate::discover::PageKind::Static => Self::Static,
            crate::discover::PageKind::Collection => Self::Collection,
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
