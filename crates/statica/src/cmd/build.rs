use std::path::Path;

use anyhow::Result;

use crate::cli::ConfigCli;
use super::util;

pub fn run(dir: &Path, overrides: &ConfigCli) -> Result<()> {
    let (root, config) = util::load_project(dir, overrides)?;
    let opts = config.to_build_options(&root);
    let report = util::run_build(&opts)?;
    util::log_build(&report, &opts.out_dir, "built");
    Ok(())
}
