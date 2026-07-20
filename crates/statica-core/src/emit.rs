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
    let mut path = out_dir.to_path_buf();
    if !route.is_empty() {
        for mut part in route.split('/') {
            if let Some((param, value)) = replace {
                let key = format!("[{param}]");
                if part == key {
                    part = value;
                }
            }
            path.push(part);
        }
    }
    path.push("index.html");
    path
}

pub fn copy_static_assets(
    root: &Path,
    out_dir: &Path,
    asset_dirs: &[String],
    process: &AssetProcessOptions,
) -> Result<ProcessReport> {
    assets::copy_asset_dirs(root, out_dir, asset_dirs, process)
}
