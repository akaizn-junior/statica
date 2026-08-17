//! statica — discover → pre → parse → layout → funnel → expand → bind → scope → emit
//!
//! # Pipeline
//!
//! 1. **Discover** — every `**/index.html` under the site root (`[param]` → collection).
//! 2. **Pre** — authoring HTML normalization before html5ever (e.g. `<slot>` in `<select>`).
//! 3. **Parse** — html5ever → owned AST; post-parse authoring lower (carriers → slots).
//! 4. **Layout** — project page head/body content into `rel="statica/layout"` shells.
//! 5. **Funnel** — load `<link rel="statica/data">` sources (JSON, JSONL/NDJSON, CSV, text, Markdown).
//! 6. **Expand** — static (1:1), collection (1:N items), or pagination (1:N page chunks).
//! 7. **Bind** — slots + `${…}` attrs + `data-t` / `data-t-{attr}` i18n + fragment/`data-each` expansion + form wiring.
//! 8. **Scope** — hash-scoped CSS/JS for fragments (CSS via lightningcss + `[data-s]`).
//! 9. **Emit** — write HTML; transform CSS to browser-ready; optional asset process + responsive images; sitemap / RSS / web manifest.
//! 10. **Minify** — optional final pass on HTML, CSS, and JS in `out_dir`.
//!
//! The `statica` CLI owns end-user config (`statica.toml`) and maps it into
//! [`BuildOptions`]. This crate does not read config files.
//!
//! # See also
//!
//! - `docs/guide.md` — authoring + config reference
//! - [`paginate`] — UI list pagination page objects
//! - [`feeds`] — sitemap + RSS (via `sitemap-rs` / `rss`)
//! - [`manifest`] — web app manifest scaffold + automatic PWA head tags

#![warn(clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::wildcard_imports,
    clippy::struct_excessive_bools,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::doc_link_with_quotes,
    clippy::needless_raw_string_hashes,
    clippy::items_after_test_module,
    clippy::items_after_statements,
    clippy::manual_let_else
)]

mod aliases;
mod assets;
mod bind;
mod build;
mod build_log;
mod content;
mod context;
pub mod css;
mod discover;
mod emit;
mod error;
mod feeds;
mod font;
mod forms;
mod fragment;
mod funnel;
mod i18n;
mod images;
mod layout;
mod loc;
mod manifest;
mod minify;
mod paginate;
pub mod parse;
mod render;
mod runtime;
mod scope;
mod search;
mod tokens;

pub use aliases::{
    join_alias, resolve_local_href, resolve_path, resolve_paths_in_document, AliasOptions,
    AliasTarget, LocalAlias, ResolvedAlias, UrlAlias,
};
pub use assets::AssetProcessOptions;
pub use build::{
    build, rebuild_paths, BuildOptions, BuildPhase, BuildReport, BuildRouteKind, BuildRouteRow,
    RenderMode,
};
pub use build_log::BuildLog;
pub use css::{transform_and_scope, transform_css};
pub use discover::PageKind;
pub use error::{Error, Result};
pub use feeds::{RssOptions, SitemapOptions};
pub use forms::{FormProvider, FormsOptions};
pub use i18n::{I18nCatalogs, I18nOptions, A11Y_TRANSLATABLE_ATTRS, DATA_T_ATTR_PREFIX};
pub use images::{ImageManifest, ImageProcessOptions, ResponsiveImage};
pub use loc::Diagnostic;
pub use manifest::{ManifestMeta, MANIFEST_FILE, MANIFEST_HREF};
pub use minify::{MinifyKind, MinifyOptions, MinifyReport};
pub use paginate::PaginationRule;
pub use parse::Document;
pub use runtime::STATICA_JS;
pub use search::SearchOptions;
