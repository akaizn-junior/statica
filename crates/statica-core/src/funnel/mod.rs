//! Build-time data funnel (JS value literals via oxc → `serde_json::Value`).

mod bind_decl;
mod js_value;
mod json;

pub use bind_decl::{
    bind_context, parse_bind_decl, validate_template_binds, BindDecl, BindSource,
};
pub use js_value::parse_js_value;
pub use json::{
    document_has_locale_data, field_as_str, find_fragment_links, find_template,
    load_data_from_document, load_locale_data_from_document, path_as_str, read_field,
    resolve_expr, strip_authoring, value_to_html, DataSource,
};
