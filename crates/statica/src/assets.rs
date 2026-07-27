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

    let results: Vec<CopyOutcome> = files
        .par_iter()
        .map(|(from, to)| copy_one_file(from, to, out_dir, process))
        .collect();

    let mut report = ProcessReport::default();
    for result in results {
        result.record(&mut report);
    }
    Ok(report)
}

fn copy_one_file(
    from: &Path,
    to: &Path,
    out_dir: &Path,
    process: &AssetProcessOptions,
) -> CopyOutcome {
    if let Some(parent) = to.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match emit_file(from, to, out_dir, process) {
        Ok(outcome) => CopyOutcome::Emitted(outcome),
        Err(e) => {
            let file = from.display().to_string();
            if let Err(copy_err) = fs::copy(from, to) {
                return CopyOutcome::Failed {
                    warning: Diagnostic::at_file(file, format!("asset copy failed: {copy_err}")),
                };
            }
            CopyOutcome::CopiedWithWarning {
                warning: Diagnostic::at_file(
                    file,
                    format!("asset process failed ({e}); copied raw"),
                ),
            }
        }
    }
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

enum CopyOutcome {
    Emitted(EmitOutcome),
    CopiedWithWarning { warning: Diagnostic },
    Failed { warning: Diagnostic },
}

impl CopyOutcome {
    fn record(self, report: &mut ProcessReport) {
        match self {
            Self::Emitted(outcome) => outcome.record(report),
            Self::CopiedWithWarning { warning } | Self::Failed { warning } => {
                report.copied += 1;
                report.warnings.push(warning);
            }
        }
    }
}

enum EmitOutcome {
    Copied,
    Processed,
    ResponsiveImage {
        key: String,
        image: crate::images::ResponsiveImage,
    },
}

impl EmitOutcome {
    fn record(self, report: &mut ProcessReport) {
        match self {
            Self::Copied => report.copied += 1,
            Self::Processed => report.processed += 1,
            Self::ResponsiveImage { key, image } => {
                report.processed += 1;
                report.images.insert(key, image);
            }
        }
    }
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
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let kind = AssetKind::from_ext(&ext);

    if !process.allows(kind) {
        fs::copy(from, to)?;
        return Ok(EmitOutcome::Copied);
    }

    match (kind, ext.as_str()) {
        (AssetKind::Css, _) => {
            let css = fs::read_to_string(from)?;
            let out = crate::minify::minify_css(&css)
                .map_err(|e| Error::at_file(from.display().to_string(), e))?;
            fs::write(to, out)?;
            Ok(EmitOutcome::Processed)
        }
        (AssetKind::Js, _) => {
            let js = fs::read_to_string(from)?;
            let out = crate::minify::minify_js(from, &js)
                .map_err(|e| Error::at_file(from.display().to_string(), e))?;
            fs::write(to, out)?;
            Ok(EmitOutcome::Processed)
        }
        (AssetKind::Image, ext) if images::is_responsive_source(ext) => {
            let resp = images::process_responsive_image(from, to, out_dir, &process.image)
                .map_err(|e| Error::at_file(from.display().to_string(), e.to_string()))?;
            let key = resp.source_url.clone();
            Ok(EmitOutcome::ResponsiveImage { key, image: resp })
        }
        // gif/svg/avif/ico and fonts: selected for processing but no transform yet → copy.
        (AssetKind::Image | AssetKind::Font | AssetKind::Other, _) => {
            fs::copy(from, to)?;
            Ok(EmitOutcome::Copied)
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
