//! Build-time data funnel (serde JSON).

mod json;

pub use json::{
    bind_context, field_as_str, find_fragment_links, find_template, is_js_identifier,
    load_data_from_document, path_as_str, read_field, resolve_expr, strip_authoring,
    value_to_html, DataSource,
};
