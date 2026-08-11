//! Embedded browser runtime (`$` scope helper).
//!
//! Production builds inline this into fragment scripts so the output needs no separate
//! statica.js fetch. Dev/preview can also serve this file as a module.

/// Source of `statica.js` — the scoped fragment script runtime.
pub const STATICA_JS: &str = include_str!("statica.js");
