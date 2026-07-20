//! Build-time data funnel (serde JSON).

mod bind_decl;
mod json;

pub use bind_decl::{
    bind_context, parse_bind_decl, validate_template_binds, BindDecl, BindSource,
};
pub use json::{
    field_as_str, find_fragment_links, find_template, load_data_from_document, path_as_str,
    read_field, resolve_expr, strip_authoring, value_to_html, DataSource,
};
