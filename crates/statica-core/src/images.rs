//! Responsive raster image processing — variants, manifest, and HTML `<picture>` wiring.
//!
//! When `[process].images` is on, raster assets (png/jpeg/webp) get width variants and
//! modern formats (WebP by default). Emitted HTML `<img>` tags pointing at those assets
//! are wrapped in `<picture>` with `srcset` / `sizes` so browsers pick an appropriate file.

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::DynamicImage;
use indexmap::IndexMap;
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::loc::Diagnostic;
use crate::parse::{self, Document, Element, Node};

/// Responsive image settings (mapped from `[process.image]` in statica.toml).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageProcessOptions {
    /// Target widths (px). Original width is always included when smaller.
    pub widths: Vec<u32>,
    /// Extra output formats besides the source format (e.g. `"webp"`).
    pub formats: Vec<String>,
    /// Lossy encode quality (jpeg / webp).
    pub quality: u8,
    /// Default `sizes` when an `<img>` has none and no `data-statica-img-sizes`.
    pub default_sizes: String,
    /// Rewrite local `<img>` tags in emitted HTML into `<picture>`.
    pub responsive: bool,
}

impl Default for ImageProcessOptions {
    fn default() -> Self {
        Self {
            widths: vec![480, 768, 1024, 1366, 1920],
            formats: vec!["webp".into()],
            quality: 85,
            default_sizes: "100vw".into(),
            responsive: true,
        }
    }
}

impl ImageProcessOptions {
    #[must_use]
    pub fn wants_format(&self, ext: &str) -> bool {
        self.formats
            .iter()
            .any(|f| f.eq_ignore_ascii_case(ext))
    }
}

/// One generated file for a width + format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageVariant {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub mime: String,
    /// Site-root URL path, e.g. `/assets/hero-768w.webp`.
    pub url: String,
}

/// Metadata for a processed source image (keyed by original `src` URL path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsiveImage {
    pub width: u32,
    pub height: u32,
    pub source_format: String,
    pub source_mime: String,
    /// Original optimized asset URL, e.g. `/assets/hero.jpg`.
    pub source_url: String,
    pub variants: Vec<ImageVariant>,
}

impl ResponsiveImage {
    #[must_use]
    pub fn variants_for_format(&self, format: &str) -> Vec<&ImageVariant> {
        let mut out: Vec<_> = self
            .variants
            .iter()
            .filter(|v| v.format.eq_ignore_ascii_case(format))
            .collect();
        out.sort_by_key(|v| v.width);
        out
    }

    #[must_use]
    pub fn srcset_for_format(&self, format: &str) -> String {
        self.variants_for_format(format)
            .iter()
            .map(|v| format!("{} {}w", v.url, v.width))
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[must_use]
    pub fn largest_for_format(&self, format: &str) -> Option<&ImageVariant> {
        self.variants_for_format(format).into_iter().last()
    }
}

/// Lookup table from normalized image `src` → responsive metadata.
#[derive(Debug, Clone, Default)]
pub struct ImageManifest {
    entries: HashMap<String, ResponsiveImage>,
}

impl ImageManifest {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn insert(&mut self, key: impl Into<String>, image: ResponsiveImage) {
        self.entries.insert(key.into(), image);
    }

    pub fn merge(&mut self, other: &ImageManifest) {
        for (k, v) in &other.entries {
            self.entries.insert(k.clone(), v.clone());
        }
    }

    #[must_use]
    pub fn get(&self, src: &str) -> Option<&ResponsiveImage> {
        let key = normalize_src(src);
        self.entries
            .get(&key)
            .or_else(|| self.entries.get(key.trim_start_matches('/')))
    }
}

/// Process a raster image: write optimized original + width/format variants beside `to`.
pub fn process_responsive_image(
    from: &Path,
    to: &Path,
    out_dir: &Path,
    opts: &ImageProcessOptions,
) -> Result<ResponsiveImage> {
    let bytes = fs::read(from).map_err(|e| Error::read(from.display().to_string(), e))?;
    let img = image::load_from_memory(&bytes)
        .map_err(|e| Error::at_file(from.display().to_string(), format!("decode: {e}")))?;

    let source_ext = from
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_else(|| "jpg".into());
    if !is_responsive_source(&source_ext) {
        return Err(Error::at_file(
            from.display().to_string(),
            "not a responsive raster format",
        ));
    }

    let (orig_w, orig_h) = (img.width(), img.height());
    let widths = target_widths(orig_w, &opts.widths);
    let source_url = url_path_for_output(out_dir, to);
    let source_mime = mime_for_ext(&source_ext);

    let mut variants = Vec::new();
    let mut output_formats: Vec<String> = Vec::new();
    if opts.wants_format("webp") && source_ext != "webp" {
        output_formats.push("webp".into());
    }
    output_formats.push(source_ext.clone());

    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }

    for &width in &widths {
        let height = scaled_height(orig_w, orig_h, width);
        let resized = if width == orig_w {
            img.clone()
        } else {
            img.resize(width, height, FilterType::Lanczos3)
        };

        for format in &output_formats {
            let encoded = encode_image(&resized, format, opts.quality)
                .map_err(|e| Error::at_file(from.display().to_string(), e))?;
            let variant_path = variant_output_path(to, width, format);
            if format == "png" {
                let optimized = oxipng::optimize_from_memory(
                    &encoded,
                    &oxipng::Options {
                        fix_errors: true,
                        ..oxipng::Options::from_preset(2)
                    },
                )
                .map_err(|e| Error::at_file(from.display().to_string(), format!("png optimize: {e}")))?;
                fs::write(&variant_path, optimized)?;
            } else {
                fs::write(&variant_path, encoded)?;
            }
            variants.push(ImageVariant {
                width,
                height,
                format: format.clone(),
                mime: mime_for_ext(format),
                url: url_path_for_output(out_dir, &variant_path),
            });
        }
    }

    // Optimized original at the canonical output path (largest width, source format).
    let largest = img.clone();
    let encoded = encode_image(&largest, &source_ext, opts.quality)
        .map_err(|e| Error::at_file(from.display().to_string(), e))?;
    let final_bytes = if source_ext == "png" {
        oxipng::optimize_from_memory(
            &encoded,
            &oxipng::Options {
                fix_errors: true,
                ..oxipng::Options::from_preset(2)
            },
        )
        .map_err(|e| Error::at_file(from.display().to_string(), format!("png optimize: {e}")))?
    } else {
        encoded
    };
    fs::write(to, final_bytes)?;

    variants.sort_by(|a, b| (a.format.as_str(), a.width).cmp(&(b.format.as_str(), b.width)));

    Ok(ResponsiveImage {
        width: orig_w,
        height: orig_h,
        source_format: source_ext,
        source_mime,
        source_url: source_url.clone(),
        variants,
    })
}

/// Rewrite `<img>` tags in HTML under `out_dir` using `manifest`.
pub fn apply_responsive_html(
    out_dir: &Path,
    manifest: &ImageManifest,
    opts: &ImageProcessOptions,
) -> Result<(usize, Vec<Diagnostic>)> {
    if !opts.responsive || manifest.is_empty() {
        return Ok((0, Vec::new()));
    }

    let mut updated = 0;
    let mut warnings = Vec::new();

    for entry in WalkDir::new(out_dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext != "html" && ext != "htm" {
            continue;
        }

        let html = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                warnings.push(Diagnostic::at_file(
                    path.display().to_string(),
                    format!("read html: {e}"),
                ));
                continue;
            }
        };

        let mut doc = match parse::parse_document(&html) {
            Ok(d) => d,
            Err(e) => {
                warnings.push(Diagnostic::at_file(path.display().to_string(), e.to_string()));
                continue;
            }
        };

        let count = transform_document_imgs(&mut doc, manifest, opts);
        if count == 0 {
            continue;
        }

        let out = parse::serialize_document(&doc);
        if let Err(e) = fs::write(path, out) {
            warnings.push(Diagnostic::at_file(
                path.display().to_string(),
                format!("write html: {e}"),
            ));
        } else {
            updated += count;
        }
    }

    Ok((updated, warnings))
}

fn transform_document_imgs(doc: &mut Document, manifest: &ImageManifest, opts: &ImageProcessOptions) -> usize {
    let mut count = 0;
    transform_nodes(&mut doc.children, manifest, opts, false, &mut count);
    count
}

fn transform_nodes(
    nodes: &mut Vec<Node>,
    manifest: &ImageManifest,
    opts: &ImageProcessOptions,
    inside_picture: bool,
    count: &mut usize,
) {
    let mut i = 0;
    while i < nodes.len() {
        let replace = if let Node::Element(el) = &nodes[i] {
            if el.name.eq_ignore_ascii_case("picture") {
                if let Node::Element(el) = &mut nodes[i] {
                    transform_nodes(&mut el.children, manifest, opts, true, count);
                }
                None
            } else if el.name.eq_ignore_ascii_case("img") && !inside_picture {
                build_picture_from_img(&nodes[i], manifest, opts)
            } else {
                if let Node::Element(el) = &mut nodes[i] {
                    transform_nodes(&mut el.children, manifest, opts, inside_picture, count);
                }
                None
            }
        } else {
            None
        };

        if let Some(picture) = replace {
            nodes[i] = picture;
            *count += 1;
        }
        i += 1;
    }
}

fn build_picture_from_img(
    node: &Node,
    manifest: &ImageManifest,
    opts: &ImageProcessOptions,
) -> Option<Node> {
    let Node::Element(img) = node else {
        return None;
    };
    if img.attr("srcset").is_some() {
        return None;
    }
    if matches!(img.attr("data-statica-img"), Some("false" | "off" | "0")) {
        return None;
    }
    let src = img.attr("src")?;
    if !is_local_src(src) {
        return None;
    }
    let resp = manifest.get(src)?;

    let sizes = img
        .attr("data-statica-img-sizes")
        .or_else(|| img.attr("sizes"))
        .unwrap_or(opts.default_sizes.as_str())
        .to_string();

    let mut picture = Element {
        name: "picture".into(),
        attrs: IndexMap::new(),
        children: Vec::new(),
        void: false,
    };

    let mut formats: Vec<String> = opts.formats.clone();
    if !formats
        .iter()
        .any(|f| f.eq_ignore_ascii_case(&resp.source_format))
    {
        formats.push(resp.source_format.clone());
    }

    for format in &formats {
        let srcset = resp.srcset_for_format(format);
        if srcset.is_empty() {
            continue;
        }
        let mut source = Element {
            name: "source".into(),
            attrs: IndexMap::new(),
            children: Vec::new(),
            void: true,
        };
        source.attrs.insert("type".into(), resp.mime_for(format).into());
        source.attrs.insert("srcset".into(), srcset);
        source.attrs.insert("sizes".into(), sizes.clone());
        picture.children.push(Node::Element(source));
    }

    let fallback = resp
        .largest_for_format(&resp.source_format)
        .map(|v| v.url.clone())
        .unwrap_or_else(|| resp.source_url.clone());
    let fallback_srcset = resp.srcset_for_format(&resp.source_format);

    let mut img_el = img.clone();
    img_el.attrs.shift_remove("data-statica-img");
    img_el.attrs.shift_remove("data-statica-img-sizes");
    img_el.attrs.insert("src".into(), fallback);
    if !fallback_srcset.is_empty() {
        img_el.attrs.insert("srcset".into(), fallback_srcset);
    }
    img_el.attrs.insert("sizes".into(), sizes);
    if !img_el.attrs.contains_key("width") {
        img_el
            .attrs
            .insert("width".into(), resp.width.to_string());
    }
    if !img_el.attrs.contains_key("height") {
        img_el
            .attrs
            .insert("height".into(), resp.height.to_string());
    }
    if !img_el.attrs.contains_key("loading") {
        img_el.attrs.insert("loading".into(), "lazy".into());
    }
    if !img_el.attrs.contains_key("decoding") {
        img_el.attrs.insert("decoding".into(), "async".into());
    }

    picture.children.push(Node::Element(img_el));
    Some(Node::Element(picture))
}

impl ResponsiveImage {
    #[must_use]
    pub fn mime_for(&self, format: &str) -> String {
        mime_for_ext(format)
    }
}

#[must_use]
pub fn is_responsive_source(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp")
}

fn target_widths(original: u32, configured: &[u32]) -> Vec<u32> {
    let mut widths: Vec<u32> = configured
        .iter()
        .copied()
        .filter(|&w| w > 0 && w <= original)
        .collect();
    if !widths.contains(&original) {
        widths.push(original);
    }
    widths.sort_unstable();
    widths.dedup();
    widths
}

fn scaled_height(orig_w: u32, orig_h: u32, new_w: u32) -> u32 {
    if orig_w == 0 {
        return orig_h;
    }
    ((u64::from(orig_h) * u64::from(new_w)) / u64::from(orig_w)).min(u64::from(u32::MAX)) as u32
}

fn variant_output_path(base: &Path, width: u32, format: &str) -> PathBuf {
    let stem = base.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{stem}-{width}w.{format}"))
}

fn url_path_for_output(out_dir: &Path, path: &Path) -> String {
    path.strip_prefix(out_dir)
        .map(|rel| format!("/{}", rel.to_string_lossy().replace('\\', "/")))
        .unwrap_or_else(|_| format!("/{}", path.to_string_lossy().replace('\\', "/")))
}

fn normalize_src(src: &str) -> String {
    let base = src.split(['?', '#']).next().unwrap_or(src).trim();
    if base.starts_with('/') {
        base.to_string()
    } else {
        format!("/{base}")
    }
}

#[must_use]
pub fn is_local_src(src: &str) -> bool {
    let s = src.trim();
    if s.is_empty() {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    !(lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("//")
        || lower.starts_with("data:")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:"))
}

fn mime_for_ext(ext: &str) -> String {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
    .into()
}

fn encode_image(img: &DynamicImage, format: &str, quality: u8) -> std::result::Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    match format.to_ascii_lowercase().as_str() {
        "png" => {
            img.write_to(&mut out, image::ImageFormat::Png)
                .map_err(|e| format!("png encode: {e}"))?;
        }
        "jpg" | "jpeg" => {
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
            img.write_with_encoder(encoder)
                .map_err(|e| format!("jpeg encode: {e}"))?;
        }
        "webp" => {
            let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
            img.write_with_encoder(encoder)
                .map_err(|e| format!("webp encode: {e}"))?;
        }
        other => return Err(format!("unsupported output format `{other}`")),
    }
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_widths_includes_original() {
        assert_eq!(target_widths(500, &[768, 1024]), vec![500]);
        assert_eq!(target_widths(2000, &[768, 1024]), vec![768, 1024, 2000]);
    }

    #[test]
    fn normalize_src_strips_query() {
        assert_eq!(normalize_src("/a.jpg?v=1"), "/a.jpg");
        assert_eq!(normalize_src("assets/a.jpg"), "/assets/a.jpg");
    }

    #[test]
    fn local_src_rejects_remote() {
        assert!(!is_local_src("https://example.com/a.jpg"));
        assert!(is_local_src("/assets/a.jpg"));
    }

    #[test]
    fn builds_picture_from_img() {
        let mut manifest = ImageManifest::default();
        manifest.insert(
            "/photo.jpg",
            ResponsiveImage {
                width: 1200,
                height: 800,
                source_format: "jpg".into(),
                source_mime: "image/jpeg".into(),
                source_url: "/photo.jpg".into(),
                variants: vec![
                    ImageVariant {
                        width: 640,
                        height: 427,
                        format: "webp".into(),
                        mime: "image/webp".into(),
                        url: "/photo-640w.webp".into(),
                    },
                    ImageVariant {
                        width: 640,
                        height: 427,
                        format: "jpg".into(),
                        mime: "image/jpeg".into(),
                        url: "/photo-640w.jpg".into(),
                    },
                    ImageVariant {
                        width: 1200,
                        height: 800,
                        format: "jpg".into(),
                        mime: "image/jpeg".into(),
                        url: "/photo-1200w.jpg".into(),
                    },
                ],
            },
        );

        let img = Node::Element(Element {
            name: "img".into(),
            attrs: IndexMap::from([
                ("src".into(), "/photo.jpg".into()),
                ("alt".into(), "Sunset".into()),
            ]),
            children: Vec::new(),
            void: true,
        });

        let opts = ImageProcessOptions::default();
        let picture = build_picture_from_img(&img, &manifest, &opts).expect("picture");
        let Node::Element(picture_el) = picture else {
            panic!("expected element");
        };
        assert_eq!(picture_el.name, "picture");
        assert_eq!(picture_el.children.len(), 3); // webp source, jpg source, img
    }

    #[test]
    fn process_responsive_image_writes_variants() {
        let dir = std::env::temp_dir().join(format!("statica-img-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let src = dir.join("in.jpg");
        let mut img = image::RgbImage::new(120, 80);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = image::Rgb([(x % 255) as u8, (y % 255) as u8, 128]);
        }
        let dyn_img = DynamicImage::ImageRgb8(img);
        let mut buf = Vec::new();
        dyn_img
            .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
            .unwrap();
        fs::write(&src, &buf).unwrap();

        let out = dir.join("out").join("photo.jpg");
        let opts = ImageProcessOptions {
            widths: vec![60, 120],
            ..ImageProcessOptions::default()
        };
        let resp = process_responsive_image(&src, &out, &dir.join("out"), &opts).unwrap();
        assert_eq!(resp.width, 120);
        assert!(out.exists());
        assert!(dir.join("out").join("photo-60w.webp").exists());
        assert!(dir.join("out").join("photo-120w.jpg").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
