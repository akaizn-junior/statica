//! statica-core — discover → pre → parse → funnel → expand → bind → scope → emit
//!
//! # Pipeline
//!
//! 1. **Discover** — every `**/index.html` under the site root (`[param]` → collection).
//! 2. **Pre** — authoring HTML normalization before html5ever (e.g. `<slot>` in `<select>`).
//! 3. **Parse** — html5ever → owned AST; post-parse authoring lower (carriers → slots).
//! 4. **Funnel** — load `<script type="statica/data">` sources (JSON, JS literals, Markdown).
//! 5. **Expand** — static (1:1), collection (1:N items), or pagination (1:N page chunks).
//! 6. **Bind** — slots + `${…}` attrs + `data-t` / `data-t-{attr}` i18n + fragment/`data-each` expansion + form wiring.
//! 7. **Scope** — hash-scoped CSS/JS for fragments (CSS via lightningcss + `[data-s]`).
//! 8. **Emit** — write HTML; transform CSS to browser-ready; optional asset process + responsive images; sitemap / RSS / web manifest.
//! 9. **Minify** — optional final pass on HTML, CSS, and JS in `out_dir`.
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
    clippy::struct_excessive_bools
)]

mod aliases;
mod assets;
mod bind;
mod build;
mod build_log;
mod content;
pub mod css;
mod discover;
mod emit;
mod error;
mod feeds;
mod font;
mod forms;
mod fragment;
mod i18n;
mod funnel;
mod images;
mod loc;
mod manifest;
mod minify;
mod paginate;
pub mod parse;
mod runtime;
mod scope;

pub use aliases::{join_alias, resolve_local_href, resolve_path, resolve_paths_in_document, AliasOptions};
pub use assets::AssetProcessOptions;
pub use build::{build, rebuild_paths, BuildOptions, BuildPhase, BuildReport, BuildRouteRow};
pub use build_log::BuildLog;
pub use css::{transform_and_scope, transform_css};
pub use discover::PageKind;
pub use error::{Error, Result};
pub use loc::Diagnostic;
pub use images::{ImageManifest, ImageProcessOptions, ResponsiveImage};
pub use minify::{MinifyKind, MinifyOptions, MinifyReport};
pub use feeds::{RssOptions, SitemapOptions};
pub use manifest::{ManifestMeta, MANIFEST_FILE, MANIFEST_HREF};
pub use forms::{FormProvider, FormsOptions};
pub use i18n::{A11Y_TRANSLATABLE_ATTRS, I18nCatalogs, I18nOptions, DATA_T_ATTR_PREFIX};
pub use paginate::PaginationRule;
pub use parse::Document;
pub use runtime::STATICA_JS;
