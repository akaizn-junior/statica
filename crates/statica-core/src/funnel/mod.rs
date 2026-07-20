//! Build-time data funnel (serde JSON).

mod json;

pub use json::{
    field_as_str, find_fragment_links, find_template, get_field, load_data_from_document,
    path_as_str, resolve_expr, strip_authoring, value_to_html, DataSource,
};
