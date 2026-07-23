//! Optional asset pipeline (`--process` / `[process]` in statica.toml).
//!
//! Per-kind toggles let you process images only, CSS only, fonts only, etc.
//!
//! - **CSS** — [lightningcss](https://lightningcss.dev/) (modern → browser-ready + minify)
//! - **JS** — [oxc](https://oxc.rs/) minifier
//! - **Images** — responsive variants + WebP ([`crate::images`]) or legacy single-file optimize
//! - **Fonts** — copied when enabled (woff/woff2/ttf/otf are already compressed containers)
//!
//! Note: `<style>` tags are always transformed by [`crate::css`] during HTML emit.
//! Linked `.css` under asset dirs are transformed when `[process].css` is on; when
//! off they are copied as-is (escape hatch for prebuilt CSS).

use std::fs;
use std::path::Path;

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::images::{self, ImageManifest, ImageProcessOptions};
use crate::loc::Diagnostic;

/// Which asset kinds to optimize when processing is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetProcessOptions {
    pub enabled: bool,
    pub css: bool,
    pub js: bool,
    pub images: bool,
    pub fonts: bool,
    pub image: ImageProcessOptions,
}

impl Default for AssetProcessOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            css: true,
            js: true,
            images: true,
            fonts: false,
            image: ImageProcessOptions::default(),
        }
    }
}

impl AssetProcessOptions {
    #[must_use]
    pub fn off() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn allows(&self, kind: AssetKind) -> bool {
        if !self.enabled {
            return false;
        }
        match kind {
            AssetKind::Css => self.css,
            AssetKind::Js => self.js,
            AssetKind::Image => self.images,
            AssetKind::Font => self.fonts,
            AssetKind::Other => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Css,
    Js,
    Image,
    Font,
    Other,
}

impl AssetKind {
    #[must_use]
    pub fn from_ext(ext: &str) -> Self {
        match ext {
            "css" => Self::Css,
            "js" | "mjs" | "cjs" => Self::Js,
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "avif" | "ico" => Self::Image,
            "woff" | "woff2" | "ttf" | "otf" | "eot" => Self::Font,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Default)]
pub struct ProcessReport {
    pub processed: usize,
    pub copied: usize,
    pub warnings: Vec<Diagnostic>,
    pub images: ImageManifest,
}

/// Copy `asset_dirs` into `out_dir`, optionally processing selected asset kinds.
pub fn copy_asset_dirs(
    root: &Path,
    out_dir: &Path,
    asset_dirs: &[String],
    process: &AssetProcessOptions,
) -> Result<ProcessReport> {
    let mut report = ProcessReport::default();
    for name in asset_dirs {
        let src = root.join(name);
        if !src.is_dir() {
            continue;
        }
        let dst = out_dir.join(name);
        let partial = copy_tree(&src, &dst, out_dir, process)?;
        report.processed += partial.processed;
        report.copied += partial.copied;
        report.warnings.extend(partial.warnings);
        report.images.merge(&partial.images);
    }
    Ok(report)
}

fn copy_tree(
    src: &Path,
    dst: &Path,
    out_dir: &Path,
    process: &AssetProcessOptions,
) -> Result<ProcessReport> {
    let mut files = Vec::new();
    collect_files(src, dst, &mut files)?;

    let results: Vec<(bool, Option<Diagnostic>, Option<(String, crate::images::ResponsiveImage)>)> =
        files
            .par_iter()
            .map(|(from, to)| {
                if let Some(parent) = to.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                match emit_file(from, to, out_dir, process) {
                    Ok(outcome) => (outcome.processed, None, outcome.manifest_entry),
                    Err(e) => {
                        let file = from.display().to_string();
                        if let Err(copy_err) = fs::copy(from, to) {
                            return (
                                false,
                                Some(Diagnostic::at_file(
                                    file.clone(),
                                    format!("asset copy failed: {copy_err}"),
                                )),
                                None,
                            );
                        }
                        (
                            false,
                            Some(Diagnostic::at_file(
                                file,
                                format!("asset process failed ({e}); copied raw"),
                            )),
                            None,
                        )
                    }
                }
            })
            .collect();

    let mut report = ProcessReport::default();
    for (did_process, warn, entry) in results {
        if did_process {
            report.processed += 1;
        } else {
            report.copied += 1;
        }
        if let Some(w) = warn {
            report.warnings.push(w);
        }
        if let Some((key, image)) = entry {
            report.images.insert(key, image);
        }
    }
    Ok(report)
}

fn collect_files(
    src: &Path,
    dst: &Path,
    out: &mut Vec<(std::path::PathBuf, std::path::PathBuf)>,
) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            collect_files(&from, &to, out)?;
        } else if ty.is_file() {
            out.push((from, to));
        }
    }
    Ok(())
}

struct EmitOutcome {
    processed: bool,
    manifest_entry: Option<(String, crate::images::ResponsiveImage)>,
}

/// Returns whether the file was transformed (not a byte-for-byte copy).
fn emit_file(
    from: &Path,
    to: &Path,
    out_dir: &Path,
    process: &AssetProcessOptions,
) -> Result<EmitOutcome> {
    let ext = from
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let kind = AssetKind::from_ext(&ext);

    if !process.allows(kind) {
        fs::copy(from, to)?;
        return Ok(EmitOutcome {
            processed: false,
            manifest_entry: None,
        });
    }

    match (kind, ext.as_str()) {
        (AssetKind::Css, _) => {
            let css = fs::read_to_string(from)?;
            let out = crate::minify::minify_css(&css)
                .map_err(|e| Error::at_file(from.display().to_string(), e))?;
            fs::write(to, out)?;
            Ok(EmitOutcome {
                processed: true,
                manifest_entry: None,
            })
        }
        (AssetKind::Js, _) => {
            let js = fs::read_to_string(from)?;
            let out = crate::minify::minify_js(from, &js)
                .map_err(|e| Error::at_file(from.display().to_string(), e))?;
            fs::write(to, out)?;
            Ok(EmitOutcome {
                processed: true,
                manifest_entry: None,
            })
        }
        (AssetKind::Image, ext) if images::is_responsive_source(ext) => {
            let resp =
                images::process_responsive_image(from, to, out_dir, &process.image).map_err(
                    |e| Error::at_file(from.display().to_string(), e.to_string()),
                )?;
            let key = resp.source_url.clone();
            Ok(EmitOutcome {
                processed: true,
                manifest_entry: Some((key, resp)),
            })
        }
        // gif/svg/avif/ico and fonts: selected for processing but no transform yet → copy.
        (AssetKind::Image | AssetKind::Font, _) => {
            fs::copy(from, to)?;
            Ok(EmitOutcome {
                processed: false,
                manifest_entry: None,
            })
        }
        (AssetKind::Other, _) => {
            fs::copy(from, to)?;
            Ok(EmitOutcome {
                processed: false,
                manifest_entry: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minifies_css() {
        let out = crate::minify::minify_css("body {  color:  #ffffff ; }").unwrap();
        assert!(out.contains("body"));
        assert!(out.len() < "body {  color:  #ffffff ; }".len());
    }

    #[test]
    fn minifies_js() {
        let out = crate::minify::minify_js(
            Path::new("x.js"),
            "const hello_world_variable = 1; console.log(hello_world_variable);",
        )
        .unwrap();
        assert!(out.contains("console"));
        assert!(out.len() < 60);
    }

    #[test]
    fn kind_gates() {
        let mut opts = AssetProcessOptions {
            enabled: true,
            css: false,
            js: false,
            images: true,
            fonts: false,
            ..AssetProcessOptions::default()
        };
        assert!(opts.allows(AssetKind::Image));
        assert!(!opts.allows(AssetKind::Css));
        opts.enabled = false;
        assert!(!opts.allows(AssetKind::Image));
    }
}
