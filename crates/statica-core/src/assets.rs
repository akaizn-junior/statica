//! Optional asset pipeline (`--process` / `[process]` in statica.toml).
//!
//! Per-kind toggles let you process images only, CSS only, fonts only, etc.
//!
//! - **CSS** — [lightningcss](https://lightningcss.dev/) (modern → browser-ready + minify)
//! - **JS** — [oxc](https://oxc.rs/) minifier
//! - **Images** — [oxipng](https://docs.rs/oxipng) + [image](https://docs.rs/image) (png/jpeg/webp)
//! - **Fonts** — copied when enabled (woff/woff2/ttf/otf are already compressed containers)
//!
//! Note: `<style>` tags are always transformed by [`crate::css`] during HTML emit.
//! Linked `.css` under asset dirs are transformed when `[process].css` is on; when
//! off they are copied as-is (escape hatch for prebuilt CSS).

use std::fs;
use std::io::Cursor;
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc_mangler::MangleOptions;
use oxc_minifier::{CompressOptions, Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;
use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::loc::Diagnostic;

/// Which asset kinds to optimize when processing is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetProcessOptions {
    pub enabled: bool,
    pub css: bool,
    pub js: bool,
    pub images: bool,
    pub fonts: bool,
}

impl Default for AssetProcessOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            css: true,
            js: true,
            images: true,
            fonts: false,
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
        let partial = copy_tree(&src, &dst, process)?;
        report.processed += partial.processed;
        report.copied += partial.copied;
        report.warnings.extend(partial.warnings);
    }
    Ok(report)
}

fn copy_tree(src: &Path, dst: &Path, process: &AssetProcessOptions) -> Result<ProcessReport> {
    let mut files = Vec::new();
    collect_files(src, dst, &mut files)?;

    let results: Vec<(bool, Option<Diagnostic>)> = files
        .par_iter()
        .map(|(from, to)| {
            if let Some(parent) = to.parent() {
                let _ = fs::create_dir_all(parent);
            }
            match emit_file(from, to, process) {
                Ok(did_process) => (did_process, None),
                Err(e) => {
                    let file = from.display().to_string();
                    if let Err(copy_err) = fs::copy(from, to) {
                        return (
                            false,
                            Some(Diagnostic::at_file(
                                file.clone(),
                                format!("asset copy failed: {copy_err}"),
                            )),
                        );
                    }
                    (
                        false,
                        Some(Diagnostic::at_file(
                            file,
                            format!("asset process failed ({e}); copied raw"),
                        )),
                    )
                }
            }
        })
        .collect();

    let mut report = ProcessReport::default();
    for (did_process, warn) in results {
        if did_process {
            report.processed += 1;
        } else {
            report.copied += 1;
        }
        if let Some(w) = warn {
            report.warnings.push(w);
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

/// Returns `true` when the file was transformed (not a byte-for-byte copy).
fn emit_file(from: &Path, to: &Path, process: &AssetProcessOptions) -> Result<bool> {
    let ext = from
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let kind = AssetKind::from_ext(&ext);

    if !process.allows(kind) {
        fs::copy(from, to)?;
        return Ok(false);
    }

    match (kind, ext.as_str()) {
        (AssetKind::Css, _) => {
            let css = fs::read_to_string(from)?;
            let out = crate::css::transform_css(&css, true).map_err(|e| Error::at_file(from.display().to_string(), e))?;
            fs::write(to, out)?;
            Ok(true)
        }
        (AssetKind::Js, _) => {
            let js = fs::read_to_string(from)?;
            let out = minify_js(from, &js).map_err(|e| Error::at_file(from.display().to_string(), e))?;
            fs::write(to, out)?;
            Ok(true)
        }
        (AssetKind::Image, "png") => {
            let bytes = fs::read(from)?;
            let out = optimize_png(&bytes).map_err(|e| Error::at_file(from.display().to_string(), e))?;
            fs::write(to, out)?;
            Ok(true)
        }
        (AssetKind::Image, "jpg" | "jpeg") => {
            let bytes = fs::read(from)?;
            let out = optimize_jpeg(&bytes).map_err(|e| Error::at_file(from.display().to_string(), e))?;
            fs::write(to, out)?;
            Ok(true)
        }
        (AssetKind::Image, "webp") => {
            let bytes = fs::read(from)?;
            let out = optimize_webp(&bytes).map_err(|e| Error::at_file(from.display().to_string(), e))?;
            fs::write(to, out)?;
            Ok(true)
        }
        // gif/svg/avif/ico and fonts: selected for processing but no transform yet → copy.
        (AssetKind::Image | AssetKind::Font, _) => {
            fs::copy(from, to)?;
            Ok(false)
        }
        (AssetKind::Other, _) => {
            fs::copy(from, to)?;
            Ok(false)
        }
    }
}

fn minify_js(path: &Path, source: &str) -> std::result::Result<String, String> {
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::mjs());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        let msg = parsed
            .diagnostics
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("js parse: {msg}"));
    }
    let mut program = parsed.program;
    let options = MinifierOptions {
        mangle: Some(MangleOptions::default()),
        compress: Some(CompressOptions::smallest()),
    };
    let ret = Minifier::new(options).minify(&allocator, &mut program);
    let code = Codegen::new()
        .with_options(CodegenOptions {
            minify: true,
            comments: CommentOptions::disabled(),
            ..CodegenOptions::default()
        })
        .with_scoping(ret.scoping)
        .build(&program)
        .code;
    Ok(code)
}

fn optimize_png(bytes: &[u8]) -> std::result::Result<Vec<u8>, String> {
    oxipng::optimize_from_memory(
        bytes,
        &oxipng::Options {
            fix_errors: true,
            ..oxipng::Options::from_preset(2)
        },
    )
    .map_err(|e| format!("png: {e}"))
}

fn optimize_jpeg(bytes: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("jpeg decode: {e}"))?;
    let mut out = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
    img.write_with_encoder(encoder)
        .map_err(|e| format!("jpeg encode: {e}"))?;
    Ok(out.into_inner())
}

fn optimize_webp(bytes: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("webp decode: {e}"))?;
    let mut out = Cursor::new(Vec::new());
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
    img.write_with_encoder(encoder)
        .map_err(|e| format!("webp encode: {e}"))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minifies_css() {
        let out = crate::css::transform_css("body {  color:  #ffffff ; }", true).unwrap();
        assert!(out.contains("body"));
        assert!(out.len() < "body {  color:  #ffffff ; }".len());
    }

    #[test]
    fn minifies_js() {
        let out = minify_js(
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
        };
        assert!(opts.allows(AssetKind::Image));
        assert!(!opts.allows(AssetKind::Css));
        opts.enabled = false;
        assert!(!opts.allows(AssetKind::Image));
    }
}
