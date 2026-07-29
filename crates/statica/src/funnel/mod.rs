//! Build-time data funnel (`serde_json::Value` from linked data files).

mod bind_decl;
mod json;

pub(crate) use bind_decl::is_identifier;
pub use bind_decl::{
    bind_context, parse_bind_decl, validate_page_template_binds,
    validate_template_binds_with_roots, BindDecl, BindSource,
};
pub use json::{
    data_link_has_locale_token, data_link_ids, document_has_locale_data, field_as_str,
    find_fragment_links, find_template, load_data_from_document, load_locale_data_from_document,
    path_as_str, path_value, read_field, resolve_expr, strip_authoring, value_to_html, DataSource,
};
