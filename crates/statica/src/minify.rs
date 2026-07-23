//! Final output minification — shrink HTML, CSS, and JS written to `out_dir`.
//!
//! Runs after pages, assets, and feeds are emitted. Inline `<style>` / `<script>`
//! are minified via the HTML pass; linked `.css` / `.js` files get a separate pass.

use std::fs;
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc_mangler::MangleOptions;
use oxc_minifier::{CompressOptions, Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;
use rayon::prelude::*;
use walkdir::WalkDir;

use crate::error::Result;
use crate::loc::Diagnostic;

/// Which output kinds to minify when processing is enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinifyOptions {
    pub enabled: bool,
    pub html: bool,
    pub css: bool,
    pub js: bool,
}

impl Default for MinifyOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            html: true,
            css: true,
            js: true,
        }
    }
}

impl MinifyOptions {
    #[must_use]
    pub fn off() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn allows(&self, kind: MinifyKind) -> bool {
        if !self.enabled {
            return false;
        }
        match kind {
            MinifyKind::Html => self.html,
            MinifyKind::Css => self.css,
            MinifyKind::Js => self.js,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinifyKind {
    Html,
    Css,
    Js,
}

impl MinifyKind {
    #[must_use]
    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext {
            "html" | "htm" => Some(Self::Html),
            "css" => Some(Self::Css),
            "js" | "mjs" | "cjs" => Some(Self::Js),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct MinifyReport {
    pub files: usize,
    pub warnings: Vec<Diagnostic>,
}

/// Minify every HTML / CSS / JS file under `out_dir` according to `opts`.
pub fn minify_output_dir(out_dir: &Path, opts: &MinifyOptions) -> Result<MinifyReport> {
    if !opts.enabled {
        return Ok(MinifyReport::default());
    }

    let files: Vec<PathBuf> = WalkDir::new(out_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let ext = e.path().extension()?.to_str()?.to_ascii_lowercase();
            MinifyKind::from_ext(&ext).map(|_| e.path().to_path_buf())
        })
        .collect();

    let results: Vec<(bool, Option<Diagnostic>)> = files
        .par_iter()
        .map(|path| match minify_file(path, opts) {
            Ok(true) => (true, None),
            Ok(false) => (false, None),
            Err(e) => (false, Some(e)),
        })
        .collect();

    let mut report = MinifyReport::default();
    for (did_minify, warn) in results {
        if did_minify {
            report.files += 1;
        }
        if let Some(w) = warn {
            report.warnings.push(w);
        }
    }
    Ok(report)
}

/// Returns `true` when the file was rewritten.
fn minify_file(path: &Path, opts: &MinifyOptions) -> std::result::Result<bool, Diagnostic> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let Some(kind) = MinifyKind::from_ext(&ext) else {
        return Ok(false);
    };
    if !opts.allows(kind) {
        return Ok(false);
    }

    let source = fs::read_to_string(path).map_err(|e| {
        Diagnostic::at_file(path.display().to_string(), format!("read failed: {e}"))
    })?;

    let out = match kind {
        MinifyKind::Html => minify_html(&source, opts),
        MinifyKind::Css => minify_css(&source).map_err(|msg| {
            Diagnostic::at_file(path.display().to_string(), msg)
        })?,
        MinifyKind::Js => minify_js(path, &source).map_err(|msg| {
            Diagnostic::at_file(path.display().to_string(), msg)
        })?,
    };

    if out == source {
        return Ok(false);
    }

    fs::write(path, out).map_err(|e| {
        Diagnostic::at_file(path.display().to_string(), format!("write failed: {e}"))
    })?;
    Ok(true)
}

#[must_use]
pub fn minify_html(source: &str, opts: &MinifyOptions) -> String {
    let mut cfg = minify_html::Cfg::default();
    cfg.minify_css = opts.css;
    cfg.minify_js = opts.js;
    let bytes = minify_html::minify(source.as_bytes(), &cfg);
    String::from_utf8_lossy(&bytes).into_owned()
}

#[must_use]
pub fn minify_css(source: &str) -> std::result::Result<String, String> {
    crate::css::transform_css(source, true)
}

pub fn minify_js(path: &Path, source: &str) -> std::result::Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn minifies_html_whitespace() {
        let opts = MinifyOptions {
            enabled: true,
            html: true,
            css: false,
            js: false,
        };
        let out = minify_html(
            "<!doctype html>\n<html>\n  <body>\n    <p> Hi </p>\n  </body>\n</html>\n",
            &opts,
        );
        assert!(out.contains("<p>"));
        assert!(out.len() < 60);
    }

    #[test]
    fn minifies_css_and_js_sources() {
        let css = minify_css("body {  color:  #ffffff ; }").unwrap();
        assert!(css.len() < "body {  color:  #ffffff ; }".len());

        let js = minify_js(
            Path::new("x.js"),
            "const hello_world_variable = 1; console.log(hello_world_variable);",
        )
        .unwrap();
        assert!(js.contains("console"));
        assert!(js.len() < 60);
    }

    #[test]
    fn walks_output_tree() {
        let dir = std::env::temp_dir().join(format!("statica-minify-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let page = dir.join("index.html");
        fs::write(
            &page,
            "<!doctype html>\n<html><body>  <p>  test  </p>  </body></html>\n",
        )
        .unwrap();
        let css = dir.join("public/site.css");
        fs::create_dir_all(css.parent().unwrap()).unwrap();
        let mut f = fs::File::create(&css).unwrap();
        writeln!(f, "body {{  margin: 0;  }}").unwrap();

        let opts = MinifyOptions {
            enabled: true,
            ..MinifyOptions::default()
        };
        let report = minify_output_dir(&dir, &opts).unwrap();
        assert_eq!(report.files, 2);
        let html = fs::read_to_string(&page).unwrap();
        assert!(!html.contains('\n') || html.len() < 50);
        let _ = fs::remove_dir_all(dir);
    }
}
