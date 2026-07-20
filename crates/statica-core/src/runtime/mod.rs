//! Embedded browser runtime (`$` scope helper).
//!
//! Production builds inline this into fragment scripts so `dist/` needs no separate
//! statica.js fetch. Dev/preview can also serve this file as a module.

/// Source of `statica.js` — the `$` action namespace.
pub const STATICA_JS: &str = include_str!("statica.js");
