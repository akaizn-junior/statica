use std::path::Path;

use anyhow::Result;

use crate::cli::ConfigCli;
use super::util;

pub fn run(dir: &Path, overrides: &ConfigCli) -> Result<()> {
    let (root, config) = util::load_project(dir, overrides)?;
    let opts = util::build_options(&config, &root, overrides, false);
    let report = util::run_build(&opts)?;
    util::log_build(&report, &opts.out_dir, "Built", opts.verbose);
    Ok(())
}
