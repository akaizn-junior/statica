//! statica-core — discover → pre → parse → funnel → expand → bind → scope → emit
//!
//! # Pipeline
//!
//! 1. **Discover** — every `**/index.html` under the site root (`[param]` → collection).
//! 2. **Pre** — authoring HTML normalization before html5ever (e.g. `<slot>` in `<select>`).
//! 3. **Parse** — html5ever → owned AST; post-parse authoring lower (carriers → slots).
//! 4. **Funnel** — load `<script type="statica/data">` JS value literals (oxc).
//! 5. **Expand** — static (1:1), collection (1:N items), or pagination (1:N page chunks).
//! 6. **Bind** — slots + `${…}` attrs + fragment/`data-each` expansion.
//! 7. **Scope** — hash-scoped CSS/JS for fragments (CSS via lightningcss + `[data-s]`).
//! 8. **Emit** — write HTML; transform CSS to browser-ready; optional asset process; sitemap / RSS.
//!
//! The `statica` CLI owns end-user config (`statica.toml`) and maps it into
//! [`BuildOptions`]. This crate does not read config files.
//!
//! # See also
//!
//! - `docs/guide.md` — authoring + config reference
//! - [`paginate`] — UI list pagination page objects
//! - [`feeds`] — sitemap + RSS (via `sitemap-rs` / `rss`)

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

mod assets;
mod bind;
mod build;
mod build_log;
pub mod css;
mod discover;
mod emit;
mod emit_opts;
mod error;
mod feeds;
mod fragment;
mod funnel;
mod loc;
mod paginate;
pub mod parse;
mod runtime;
mod scope;

pub use assets::AssetProcessOptions;
pub use build::{build, rebuild_paths, BuildOptions, BuildPhase, BuildReport, BuildRouteRow};
pub use build_log::BuildLog;
pub use css::{transform_and_scope, transform_css};
pub use discover::PageKind;
pub use emit_opts::EmitOptions;
pub use error::{Error, Result};
pub use loc::Diagnostic;
pub use feeds::{RssOptions, SitemapOptions};
pub use paginate::PaginationRule;
pub use parse::Document;
pub use runtime::STATICA_JS;
