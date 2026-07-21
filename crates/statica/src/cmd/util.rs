//! Shared CLI helpers: project resolution, build logging.
//!
//! # Project resolution
//!
//! [`load_project`] is the entry point for `build` / `serve` / `watch`:
//!
//! 1. Resolve `PATH` against the **process cwd** ([`resolve_against_cwd`]).
//! 2. Walk up for `statica.toml` ([`find_config_dir`]).
//! 3. Load config and apply CLI overrides.
//! 4. Honor `project` / `--project` as a subdirectory ([`site_root`]).
//!
//! Relative paths are never resolved against the binary install location.

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use statica_core::{build, rebuild_paths, BuildOptions, BuildReport, BuildRouteRow, Diagnostic, PageKind};

use crate::cli::ConfigCli;
use crate::config::{StaticaConfig, CONFIG_FILE};
use crate::style;

/// Locate site root + loaded config from a CLI path (default `.` = cwd).
///
/// Always resolves relative paths against the **process cwd** (not the binary path).
/// Walks up for `statica.toml`, applies CLI overrides, then honors config/`--project`.
pub fn load_project(dir: &Path, overrides: &ConfigCli) -> Result<(PathBuf, StaticaConfig)> {
    let start = resolve_against_cwd(dir)?;
    let config_dir = find_config_dir(&start).unwrap_or_else(|| start.clone());
    let mut config = StaticaConfig::load(&config_dir)?;
    config.apply_env(&config_dir)?;
    config.apply_cli(overrides)?;
    let root = site_root(&config_dir, &config)?;
    Ok((root, config))
}

/// Absolute path for `dir`, always relative to cwd when not absolute.
pub fn resolve_against_cwd(dir: &Path) -> Result<PathBuf> {
    let cwd = env::current_dir().context("could not read current working directory (cwd)")?;
    let joined = if dir.as_os_str().is_empty() || dir == Path::new(".") {
        cwd
    } else if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        cwd.join(dir)
    };
    resolve_existing(&joined)
        .with_context(|| format!("could not resolve project path `{}` (cwd {})", dir.display(), env::current_dir().map(|p| p.display().to_string()).unwrap_or_default()))
}

/// Site root after config is loaded (applies `project` subpath).
pub fn site_root(config_dir: &Path, config: &StaticaConfig) -> Result<PathBuf> {
    let sub = config.project.trim();
    if sub.is_empty() {
        return Ok(config_dir.to_path_buf());
    }
    let candidate = if Path::new(sub).is_absolute() {
        PathBuf::from(sub)
    } else {
        config_dir.join(sub)
    };
    resolve_existing(&candidate).with_context(|| {
        format!(
            "config `project = \"{sub}\"` (from {}) does not exist",
            config_dir.join(CONFIG_FILE).display()
        )
    })
}

fn resolve_existing(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        bail!("path does not exist: {}", path.display());
    }
    let path = if path.is_file() {
        if path.file_name().is_some_and(|n| n == CONFIG_FILE) {
            path.parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| anyhow::anyhow!("invalid project path {}", path.display()))?
        } else {
            bail!(
                "expected a project directory (or {}), got file {}",
                CONFIG_FILE,
                path.display()
            );
        }
    } else {
        path.to_path_buf()
    };
    path.canonicalize()
        .with_context(|| format!("could not canonicalize `{}`", path.display()))
}

/// Walk up from `start` (inclusive) looking for `statica.toml`.
pub fn find_config_dir(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join(CONFIG_FILE).is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

pub fn print_warnings(warnings: &[Diagnostic]) {
    for w in warnings {
        eprintln!("{} {w}", style::warn("warning:"));
    }
}

pub fn run_build(opts: &BuildOptions) -> Result<BuildReport> {
    build(opts).context("build failed")
}

pub fn run_rebuild(opts: &BuildOptions, changed: &[PathBuf]) -> Result<BuildReport> {
    rebuild_paths(opts, changed).context("rebuild failed")
}

pub fn build_options(
    config: &crate::config::StaticaConfig,
    root: &Path,
    cli: &ConfigCli,
    default_verbose: bool,
) -> BuildOptions {
    let mut opts = config.to_build_options(root);
    opts.verbose = !cli.silent && (cli.verbose || default_verbose);
    opts
}

pub fn log_build(report: &BuildReport, out_dir: &Path, verb: &str, verbose: bool) {
    print_warnings(&report.warnings);

    if report.pages_written == 0 && report.duration_ms == 0 {
        return;
    }

    if !verbose {
        return;
    }

    if !report.routes.is_empty() {
        eprintln!();
        print_route_table(&report.routes);
    }

    if !report.phases.is_empty() {
        eprintln!();
        for phase in &report.phases {
            eprintln!(
                "  {}  {} ({}{})",
                style::dim(phase.name),
                phase.detail,
                phase.duration_ms,
                style::dim("ms"),
            );
        }
    }

    eprintln!();
    if report.assets_processed > 0 {
        eprintln!(
            "{} {} page(s), {} asset(s) → {} in {}",
            style::success(format!("{verb}")),
            style::bold(report.pages_written.to_string()),
            style::bold(report.assets_processed.to_string()),
            style::dim(out_dir.display().to_string()),
            style::dim(format!("{}ms", report.duration_ms)),
        );
    } else {
        eprintln!(
            "{} {} page(s) → {} in {}",
            style::success(format!("{verb}")),
            style::bold(report.pages_written.to_string()),
            style::dim(out_dir.display().to_string()),
            style::dim(format!("{}ms", report.duration_ms)),
        );
    }
}

fn route_type_label(row: &BuildRouteRow) -> (&'static str, &'static str) {
    if row.paginated {
        ("◐", "paginated")
    } else {
        match row.kind {
            PageKind::Static => ("○", "static"),
            PageKind::Collection => ("λ", "collection"),
        }
    }
}

fn print_route_table(routes: &[BuildRouteRow]) {
    let route_w = routes
        .iter()
        .map(|r| r.route.len())
        .max()
        .unwrap_or(5)
        .max(5);
    let type_w = 10;

    eprintln!(
        "{:<route_w$}  {:<type_w$}  {}",
        style::bold("Route"),
        style::bold("Type"),
        style::bold("Pages"),
        route_w = route_w,
        type_w = type_w,
    );

    for row in routes {
        let (symbol, label) = route_type_label(row);
        let route = if row.route.is_empty() {
            "/".to_string()
        } else {
            row.route.clone()
        };
        eprintln!(
            "{:<route_w$}  {:<type_w$}  {}",
            format!("{symbol} {route}"),
            label,
            row.pages,
            route_w = route_w + 2,
            type_w = type_w,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_tree() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "statica-resolve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("nested/deep")).unwrap();
        fs::write(dir.join("statica.toml"), "out_dir = \".dist\"\n").unwrap();
        dir
    }

    #[test]
    fn finds_toml_walking_up() {
        let root = temp_tree();
        let deep = root.join("nested/deep").canonicalize().unwrap();
        let found = find_config_dir(&deep).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_subdir_from_config() {
        let root = temp_tree();
        let site = root.join("site");
        fs::create_dir_all(&site).unwrap();
        fs::write(site.join("index.html"), "<!doctype html><html></html>").unwrap();
        fs::write(
            root.join("statica.toml"),
            "project = \"site\"\nout_dir = \".dist\"\n",
        )
        .unwrap();
        let cli = ConfigCli::default();
        let (resolved, cfg) = load_project(&root, &cli).unwrap();
        assert_eq!(resolved, site.canonicalize().unwrap());
        assert_eq!(cfg.project, "site");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cli_project_overrides_toml() {
        let root = temp_tree();
        let site = root.join("site");
        fs::create_dir_all(&site).unwrap();
        fs::write(
            root.join("statica.toml"),
            "project = \"missing\"\nout_dir = \".dist\"\n",
        )
        .unwrap();
        let cli = ConfigCli {
            project: Some("site".into()),
            ..ConfigCli::default()
        };
        let (resolved, _) = load_project(&root, &cli).unwrap();
        assert_eq!(resolved, site.canonicalize().unwrap());
        let _ = fs::remove_dir_all(root);
    }
}
