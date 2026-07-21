//! Write rendered HTML and copy/process static asset folders.

use std::fs;
use std::path::{Path, PathBuf};

use crate::assets::{self, AssetProcessOptions, ProcessReport};
use crate::error::Result;

pub fn write_html(path: &Path, html: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, html)?;
    Ok(())
}

#[must_use]
pub fn out_path_for_route(
    out_dir: &Path,
    route: &str,
    replace: Option<(&str, &str)>,
) -> PathBuf {
    match replace {
        Some(pair) => out_path_for_route_replacements(out_dir, route, &[pair]),
        None => out_path_for_route_replacements(out_dir, route, &[]),
    }
}

#[must_use]
pub fn out_path_for_route_replacements(
    out_dir: &Path,
    route: &str,
    replacements: &[(&str, &str)],
) -> PathBuf {
    let mut path = out_dir.to_path_buf();
    if !route.is_empty() {
        for part in route.split('/') {
            let mut segment = part;
            for (param, value) in replacements {
                if segment == format!("[{param}]") {
                    segment = value;
                    break;
                }
            }
            path.push(segment);
        }
    }
    path.push("index.html");
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_multiple_params() {
        let out = out_path_for_route_replacements(
            Path::new("/dist"),
            "[locale]/posts/[slug]",
            &[("locale", "pt"), ("slug", "hello")],
        );
        assert_eq!(out, Path::new("/dist/pt/posts/hello/index.html"));
    }
}

pub fn copy_static_assets(
    root: &Path,
    out_dir: &Path,
    asset_dirs: &[String],
    process: &AssetProcessOptions,
) -> Result<ProcessReport> {
    assets::copy_asset_dirs(root, out_dir, asset_dirs, process)
}
