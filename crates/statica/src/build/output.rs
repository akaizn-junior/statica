//! Final HTML output helpers owned by the build pipeline.

use std::path::Path;

use crate::emit;
use crate::error::Result;
use crate::search;

use super::BuildOptions;

pub(super) fn ensure_default_404(out_dir: &Path) -> Result<()> {
    let flat = out_dir.join("404.html");
    let nested = out_dir.join("404").join("index.html");
    if flat.exists() || nested.exists() {
        return Ok(());
    }
    emit::write_html(&nested, default_404_html())?;
    Ok(())
}

pub(super) fn write_rendered_html(opts: &BuildOptions, path: &Path, html: &str) -> Result<()> {
    let (html, _) = search::rewrite_controls(html, &opts.search)?;
    emit::write_html(path, &html)?;
    Ok(())
}

fn default_404_html() -> &'static str {
    include_str!("../runtime/404.html")
}
