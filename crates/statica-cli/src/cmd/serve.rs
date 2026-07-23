//! Serve a previously built site (no rebuild).

use std::path::Path;

use anyhow::Result;

use crate::cli::ConfigCli;
use crate::style;
use super::{preview, util};

pub async fn run(dir: &Path, overrides: &ConfigCli) -> Result<()> {
    let (root, config) = util::load_project(dir, overrides)?;
    let opts = config.to_build_options(&root);
    let host = config.preview.host_addr()?;
    let port = config.preview.port;

    eprintln!(
        "{} {}",
        style::accent("statica serve"),
        style::dim(root.display().to_string()),
    );

    preview::serve_dir(&opts.out_dir, host, port).await
}
