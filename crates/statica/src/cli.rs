use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

pub use crate::cli_config::ConfigCli;

/// statica — Powered HTML
#[derive(Parser, Debug)]
#[command(
    name = "statica",
    version,
    about = "statica — Powered HTML",
    long_about = LONG_ABOUT,
    after_long_help = AFTER_LONG_HELP,
    disable_version_flag = true,
    disable_help_subcommand = true,
    args_conflicts_with_subcommands = true,
    propagate_version = true,
)]
pub struct Cli {
    /// Print version and exit
    #[arg(
        short = 'v',
        long = "version",
        action = ArgAction::SetTrue,
        global = true,
        help = "Print version and exit",
        long_help = "Print the statica version (from Cargo.toml) and exit successfully."
    )]
    pub version: bool,

    /// Overrides for every `statica.toml` key (CLI wins).
    #[command(flatten)]
    pub config: ConfigCli,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Project directory to build when no subcommand is given
    #[arg(
        value_name = "PATH",
        default_value = ".",
        help = "Project directory (default command: build)",
        long_help = "Project root to start from (default: current working directory).\n\
statica resolves this path against cwd, then walks up looking for `statica.toml`.\n\
The site root is that config directory, or `project` / `--project` under it."
    )]
    pub path: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Build the site into the configured output directory
    #[command(
        about = "Build Funnel → pages → output directory",
        long_about = BUILD_ABOUT,
        after_long_help = BUILD_AFTER
    )]
    Build(PathArgs),

    /// Serve a previously built output directory
    #[command(
        about = "Serve out_dir over HTTP (preview a build)",
        long_about = SERVE_ABOUT,
        after_long_help = SERVE_AFTER
    )]
    Serve(PathArgs),

    /// Watch for changes, rebuild, and serve the output directory
    #[command(
        about = "Watch, rebuild, and serve a local preview",
        long_about = WATCH_ABOUT,
        after_long_help = WATCH_AFTER
    )]
    Watch(PathArgs),

    /// Scaffold a new statica project
    #[command(
        about = "Create a new statica project scaffold",
        long_about = NEW_ABOUT,
        after_long_help = NEW_AFTER
    )]
    New(NewArgs),
}

#[derive(Args, Debug)]
pub struct PathArgs {
    /// Project directory
    #[arg(
        value_name = "PATH",
        default_value = ".",
        help = "Start directory (default: cwd)",
        long_help = "Directory to start from when locating the project. Resolved against the process cwd.\n\
Walks up for `statica.toml`. Use `--project` (or `project` in the toml) for a site subdirectory."
    )]
    pub path: PathBuf,
}

#[derive(Args, Debug)]
pub struct NewArgs {
    /// New project directory name
    #[arg(
        value_name = "NAME",
        help = "Directory / project name to create",
        long_help = "Name of the directory to create. Must not already exist.\n\
Writes a starter site (pages, fragments, sample funnel JSON) and a documented `statica.toml`."
    )]
    pub name: String,
}

const LONG_ABOUT: &str = "\
statica — Powered HTML

Turns HTML pages, fragments, and local JSON funnels into a static site.
Transforms modern CSS to browser-ready output.

Flow: Funnel → Pages → output directory (default: .dist)

With no subcommand, statica builds PATH (default: `.`):
  statica
  statica examples/blog
  statica serve examples/blog
  statica watch examples/blog

End-user settings are read from statica.toml (optional). Every key has an
equivalent CLI flag; flags override the file.

Project location:
  PATH (default `.`) is resolved against the process cwd, then statica walks
  up for statica.toml. Site root = that directory, or `project` / `--project`
  under it.";

const AFTER_LONG_HELP: &str = "\
Examples:
  statica                         Build cwd (find statica.toml walking up)
  statica build examples/blog
  statica build --project site    Use project=site under found statica.toml
  statica build --process         Optimize public/ CSS, JS, images
  statica build --minify          Shrink HTML, CSS, JS in out_dir
  statica --site-url https://x.com --rss
  statica serve ./site            Preview .dist over HTTP
  statica watch . --port 8080     Watch, rebuild, serve on :8080
  statica new my-site             Scaffold my-site/ with statica.toml

Configuration:
  Nested tables use compact SPECs (same style as pagination):
    --rss 'title=Blog,limit=20,collections=posts'
    --sitemap 'filename=sitemap.xml,urls_per_file=50000'
    --process 'css=true,js=false,images=true'
    --minify 'html=true,css=true,js=true'
    --preview host=127.0.0.1,port=9000
    --pagination 'route=blog/[page],page_size=10,sort_desc=true,index=true'
    --i18n 'locales=en|pt,default=en'
  (collections in --rss use | as list separator)

Exit status:
  0 on success; non-zero on build, I/O, or config errors.

See also:
  docs/guide.md, man statica(1)";

const BUILD_ABOUT: &str = "\
Build Funnel → pages → output directory.

Discovers every **/index.html under PATH, loads funnels and fragments,
renders pages (including [slug] collections and [[pagination]]), and writes
HTML to out_dir (from statica.toml / flags, default .dist).";

const BUILD_AFTER: &str = "\
Examples:
  statica build
  statica build examples/blog
  statica build --process --no-sitemap --rss-limit 20
  statica build --verbose             Step logs + route summary

Notes:
  Full builds clean out_dir when clean = true (default; --no-clean to keep).
  Asset folders listed in asset_dirs are copied when copy_assets = true.
  Pass --process to enable [process] (kinds: css, js, images, fonts).
  Pass --minify to enable [minify] (kinds: html, css, js).";

const SERVE_ABOUT: &str = "\
Serve a previously built out_dir over HTTP.

Uses axum + tower-http ServeDir: directory indexes, precompressed gzip,
and index.html fallback. Does not rebuild — run `statica build` first
(or use `statica watch` to build continuously).";

const SERVE_AFTER: &str = "\
Examples:
  statica build && statica serve
  statica serve examples/blog
  statica serve . -p 9000 --host 127.0.0.1

Notes:
  --host / --port override [preview] from statica.toml.
  Defaults: host 0.0.0.0, port 4321.
  Prints Local + Network (LAN) URLs for phone testing.
  Fails if out_dir is missing.";

const WATCH_ABOUT: &str = "\
Watch PATH for changes, rebuild incrementally, and serve out_dir over HTTP.

Performs an initial full build, then rebuilds when source files change.
Debounce and poll intervals come from [preview] (or --debounce-ms /
--poll-interval-secs). Preview uses the same stack as `statica serve`.";

const WATCH_AFTER: &str = "\
Examples:
  statica watch
  statica watch examples/blog
  statica watch . -p 9000 --process

Notes:
  Build logs are shown by default; pass `--silent` to hide them.
  Use `statica build --verbose` for one-off builds with logs.
  --host / --port / --process and other config flags override statica.toml.
  Defaults: host 0.0.0.0, port 4321.
  Prints Local + Network (LAN) URLs for phone testing.";

const NEW_ABOUT: &str = "\
Create a new project directory with a starter site and statica.toml.

Scaffolds content/, ui/ fragments, listing and collection pages, and a
documented config file with defaults.";

const NEW_AFTER: &str = "\
Examples:
  statica new my-site
  cd my-site && statica
  statica serve my-site
  statica watch my-site

Fails if NAME already exists.";
