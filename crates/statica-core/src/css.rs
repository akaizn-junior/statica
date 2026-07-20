//! Builtin CSS transform via [lightningcss](https://lightningcss.dev/).
//!
//! Authors write modern CSS (nesting, range media, modern colors, …). statica
//! compiles it to browser-ready CSS — no user PostCSS config.
//!
//! Used for linked stylesheets under asset dirs and for `<style>` in HTML
//! (including fragment-scoped styles).

use lightningcss::stylesheet::{
    MinifyOptions, ParserFlags, ParserOptions, PrinterOptions, StyleSheet,
};
use lightningcss::targets::{Features, Targets};

/// Compile modern CSS → browser-ready CSS.
///
/// Enables nesting + custom media parsing, then compiles nesting, media range
/// syntax, modern colors, logical properties, and related features.
#[must_use]
pub fn transform_css(source: &str, minify: bool) -> Result<String, String> {
    let mut flags = ParserFlags::empty();
    flags.insert(ParserFlags::NESTING);
    flags.insert(ParserFlags::CUSTOM_MEDIA);

    let mut stylesheet = StyleSheet::parse(
        source,
        ParserOptions {
            flags,
            error_recovery: false,
            ..ParserOptions::default()
        },
    )
    .map_err(|e| format!("css parse: {e}"))?;

    let targets = default_targets();
    stylesheet
        .minify(MinifyOptions {
            targets,
            ..MinifyOptions::default()
        })
        .map_err(|e| format!("css transform: {e}"))?;

    let res = stylesheet
        .to_css(PrinterOptions {
            minify,
            targets,
            ..PrinterOptions::default()
        })
        .map_err(|e| format!("css print: {e}"))?;
    Ok(res.code)
}

/// Flatten modern CSS, apply `[data-s="…"]` scoping, then minify.
pub fn transform_and_scope(source: &str, scope_id: &str) -> Result<String, String> {
    let flat = transform_css(source, false)?;
    let scoped = crate::scope::scope_style_text(&flat, scope_id);
    transform_css(&scoped, true)
}

fn default_targets() -> Targets {
    // No browserslist file — always compile these so output is widely usable.
    Targets {
        include: Features::Nesting
            | Features::MediaQueries
            | Features::Colors
            | Features::LogicalProperties
            | Features::Selectors
            | Features::ClampFunction
            | Features::LightDark
            | Features::DoublePositionGradients
            | Features::FontFamilySystemUi
            | Features::TextDecorationThicknessPercent
            | Features::VendorPrefixes,
        ..Targets::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_nesting() {
        let out = transform_css(
            r#"
            .card {
              color: red;
              & h2 { color: blue; }
            }
            "#,
            true,
        )
        .unwrap();
        assert!(out.contains(".card"));
        assert!(out.contains("h2") || out.contains(".card h2"));
        assert!(!out.contains('&'), "nesting should be compiled away: {out}");
    }

    #[test]
    fn compiles_media_range() {
        let out = transform_css(
            r#"
            @media (width >= 40rem) {
              .x { padding: 1rem; }
            }
            "#,
            true,
        )
        .unwrap();
        assert!(out.contains(".x"));
        assert!(
            out.contains("min-width") || out.contains("width>="),
            "expected range media compiled or preserved: {out}"
        );
    }

    #[test]
    fn scope_after_nesting() {
        let out = transform_and_scope(
            r#"
            .card {
              color: red;
              & .title { font-weight: 700; }
            }
            "#,
            "frag-1",
        )
        .unwrap();
        assert!(
            out.contains("[data-s=\"frag-1\"]") || out.contains("[data-s=frag-1]"),
            "{out}"
        );
        assert!(!out.contains('&'), "{out}");
    }
}
