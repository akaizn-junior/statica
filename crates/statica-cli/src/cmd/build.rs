use std::path::Path;

use anyhow::Result;

use super::util;
use crate::cli::ConfigCli;

pub fn run(dir: &Path, overrides: &ConfigCli) -> Result<()> {
    let (root, config) = util::load_project(dir, overrides)?;
    let opts = util::build_options(&config, &root, overrides, false);
    let report = util::run_build(&opts)?;
    util::log_build(&report, &opts.out_dir, "Built", opts.verbose);
    util::write_report_json(&report, overrides.report_json.as_deref())?;
    Ok(())
}
