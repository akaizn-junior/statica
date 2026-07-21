//! CLI flag overrides that mirror every `statica.toml` key (clap definitions only).
//!
//! # SPEC format
//!
//! Nested tables (`[emit]`, `[process]`, `[sitemap]`, `[rss]`, `[preview]`,
//! `[[pagination]]`) are overridden with compact `key=value,key=value` strings,
//! the same style as `--pagination`.
//!
//! - Bare `--rss` / `--sitemap` / `--process` enable that feature (empty SPEC).
//! - `--no-rss` / `--no-sitemap` / `--no-process` disable.
//! - Inside `--rss`, list fields use `|` (`collections=posts|notes`) because
//!   `,` separates keys.
//!
//! Parsing and application live in [`crate::config::StaticaConfig::apply_cli`].

use clap::{ArgAction, Args};

/// Flags that override `statica.toml`. Absent → keep file / defaults.
///
/// Flattened onto the root [`crate::cli::Cli`] so flags work for the default
/// build command and all subcommands (`global = true`).
#[derive(Args, Debug, Clone, Default)]
pub struct ConfigCli {
    // ── project location ───────────────────────────────────────
    #[arg(
        long = "project",
        value_name = "DIR",
        global = true,
        help = "Site root relative to statica.toml (project)",
        long_help = "Overrides `project` in statica.toml.\n\
Path is relative to the directory that contains `statica.toml` (found by walking up from cwd / PATH).\n\
Empty / omitted = that config directory is the site root."
    )]
    pub project: Option<String>,

    // ── top-level ──────────────────────────────────────────────
    #[arg(long = "out-dir", value_name = "DIR", global = true, help = "Output directory (out_dir)")]
    pub out_dir: Option<String>,

    #[arg(long = "site-url", value_name = "URL", global = true, help = "Site origin for sitemap/RSS (site_url)")]
    pub site_url: Option<String>,

    #[arg(long = "clean", action = ArgAction::SetTrue, global = true, overrides_with = "no_clean", help = "Clean out_dir before build")]
    pub clean: bool,
    #[arg(long = "no-clean", action = ArgAction::SetTrue, global = true, help = "Do not clean out_dir before build")]
    pub no_clean: bool,

    #[arg(long = "copy-assets", action = ArgAction::SetTrue, global = true, overrides_with = "no_copy_assets", help = "Copy asset_dirs into out_dir")]
    pub copy_assets: bool,
    #[arg(long = "no-copy-assets", action = ArgAction::SetTrue, global = true, help = "Skip copying asset directories")]
    pub no_copy_assets: bool,

    #[arg(long = "asset-dirs", value_name = "DIRS", global = true, value_delimiter = ',', help = "Comma-separated asset folder names")]
    pub asset_dirs: Option<Vec<String>>,

    #[arg(long = "ignore-dirs", value_name = "DIRS", global = true, value_delimiter = ',', help = "Comma-separated dirs to skip when discovering pages")]
    pub ignore_dirs: Option<Vec<String>>,

    // ── [emit] SPEC ────────────────────────────────────────────
    #[arg(
        long = "emit",
        value_name = "SPEC",
        global = true,
        help = "Emit options as key=value SPEC",
        long_help = "Override [emit]. Comma-separated key=value:\n\
  strip_data, strip_fragments, strip_html_data_bind, dedupe_helpers, dedupe_styles\n\
Example: --emit strip_data=false,dedupe_styles=true"
    )]
    pub emit: Option<String>,

    // ── [process] SPEC ─────────────────────────────────────────
    #[arg(
        long = "process",
        value_name = "SPEC",
        num_args = 0..=1,
        default_missing_value = "",
        global = true,
        help = "Enable process; optional key=value SPEC",
        long_help = "Enable [process]. Optional SPEC:\n\
  enabled, css, js, images, fonts\n\
Examples:\n\
  --process\n\
  --process css=true,js=false,images=true,fonts=false"
    )]
    pub process: Option<String>,
    #[arg(long = "no-process", action = ArgAction::SetTrue, global = true, help = "Disable asset processing")]
    pub no_process: bool,

    // ── [minify] SPEC ──────────────────────────────────────────
    #[arg(
        long = "minify",
        value_name = "SPEC",
        num_args = 0..=1,
        default_missing_value = "",
        global = true,
        help = "Enable final minify; optional key=value SPEC",
        long_help = "Enable [minify] — shrink HTML, CSS, and JS in out_dir.\n\
  enabled, html, css, js\n\
Examples:\n\
  --minify\n\
  --minify html=true,css=true,js=false"
    )]
    pub minify: Option<String>,
    #[arg(long = "no-minify", action = ArgAction::SetTrue, global = true, help = "Disable final minification")]
    pub no_minify: bool,

    // ── [sitemap] SPEC ─────────────────────────────────────────
    #[arg(
        long = "sitemap",
        value_name = "SPEC",
        num_args = 0..=1,
        default_missing_value = "",
        global = true,
        help = "Enable sitemap; optional key=value SPEC",
        long_help = "Enable [sitemap]. Optional SPEC:\n\
  enabled, filename, urls_per_file\n\
Examples:\n\
  --sitemap\n\
  --sitemap filename=sitemap.xml,urls_per_file=50000"
    )]
    pub sitemap: Option<String>,
    #[arg(long = "no-sitemap", action = ArgAction::SetTrue, global = true, help = "Disable sitemap generation")]
    pub no_sitemap: bool,

    // ── [rss] SPEC ─────────────────────────────────────────────
    #[arg(
        long = "rss",
        value_name = "SPEC",
        num_args = 0..=1,
        default_missing_value = "",
        global = true,
        help = "Enable RSS; optional key=value SPEC",
        long_help = "Enable [rss]. Optional SPEC:\n\
  enabled, filename, title, description, language, limit,\n\
  title_field, description_field, date_field, collections\n\
Examples:\n\
  --rss\n\
  --rss 'title=Blog,limit=20,collections=posts,date_field=published_at'"
    )]
    pub rss: Option<String>,
    #[arg(long = "no-rss", action = ArgAction::SetTrue, global = true, help = "Disable RSS feed")]
    pub no_rss: bool,

    // ── [[pagination]] SPEC ────────────────────────────────────
    #[arg(
        long = "pagination",
        value_name = "SPEC",
        global = true,
        action = ArgAction::Append,
        help = "Pagination rule SPEC (repeatable; replaces [[pagination]])",
        long_help = "Replace all [[pagination]] entries. Repeat for multiple rules.\n\
SPEC is comma-separated key=value:\n\
  route (required), page_size|per_page, limit, offset,\n\
  sort_by, sort_desc, max_pages, index\n\
Example: --pagination 'route=blog/[page],page_size=10,sort_desc=true,index=true'"
    )]
    pub pagination: Vec<String>,

    // ── [preview] SPEC (+ short host/port) ─────────────────────
    #[arg(
        long = "preview",
        value_name = "SPEC",
        global = true,
        help = "Preview options as key=value SPEC",
        long_help = "Override [preview]. Comma-separated key=value:\n\
  host, port, debounce_ms, poll_interval_secs\n\
Example: --preview host=127.0.0.1,port=9000,debounce_ms=100\n\
`--host` / `-p` are short aliases for the same fields."
    )]
    pub preview: Option<String>,

    #[arg(long = "host", value_name = "HOST", global = true, help = "Preview bind address (alias for --preview host=…)")]
    pub host: Option<String>,

    #[arg(short = 'p', long = "port", value_name = "PORT", global = true, help = "Preview server port (alias for --preview port=…)")]
    pub port: Option<u16>,

    // ── [i18n] SPEC ────────────────────────────────────────────
    #[arg(
        long = "i18n",
        value_name = "SPEC",
        num_args = 0..=1,
        default_missing_value = "",
        global = true,
        help = "Enable i18n; optional key=value SPEC",
        long_help = "Enable [i18n]. Optional SPEC:\n\
  enabled, default, locales, dir, fallback\n\
Examples:\n\
  --i18n\n\
  --i18n 'locales=en|pt,default=en'"
    )]
    pub i18n: Option<String>,
    #[arg(long = "no-i18n", action = ArgAction::SetTrue, global = true, help = "Disable i18n")]
    pub no_i18n: bool,

    /// Show build step logs and a route summary (silent by default on build).
    #[arg(
        long = "verbose",
        short = 'V',
        action = ArgAction::SetTrue,
        global = true,
        conflicts_with = "silent",
        help = "Show build logs and summary",
        long_help = "Print pipeline step timings during the build and a Next.js-style route table at the end.\n\
`statica build` is silent by default; `statica watch` shows logs by default."
    )]
    pub verbose: bool,

    /// Suppress build logs (overrides watch default verbosity).
    #[arg(
        long = "silent",
        action = ArgAction::SetTrue,
        global = true,
        conflicts_with = "verbose",
        help = "Suppress build logs",
        long_help = "Hide build step logs and the route summary.\n\
`statica watch` shows logs by default; pass `--silent` to turn them off."
    )]
    pub silent: bool,
}
