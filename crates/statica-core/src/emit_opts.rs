//! Emit / authoring-strip options (mapped from `[emit]` in statica.toml).

/// What to remove or tidy when writing HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitOptions {
    /// Remove `<script type="statica/data">` from output.
    pub strip_data: bool,
    /// Remove `<link rel="statica/fragment">` from output.
    pub strip_fragments: bool,
    /// Remove `data-bind` from the root `<html>` element.
    pub strip_html_data_bind: bool,
    /// Dedupe inlined `$` / statica helper scripts across the document.
    pub dedupe_helpers: bool,
    /// Dedupe scoped `<style>` blocks across the document.
    pub dedupe_styles: bool,
}

impl Default for EmitOptions {
    fn default() -> Self {
        Self {
            strip_data: true,
            strip_fragments: true,
            strip_html_data_bind: true,
            dedupe_helpers: true,
            dedupe_styles: true,
        }
    }
}
