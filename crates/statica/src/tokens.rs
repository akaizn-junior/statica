//! Core statica authoring and runtime contract tokens.

pub(crate) const REL_DATA: &str = "statica/data";
pub(crate) const REL_FRAGMENT: &str = "statica/fragment";
pub(crate) const REL_FONT: &str = "statica/font";
pub(crate) const TYPE_SEARCH: &str = "statica/search";
pub(crate) const TYPE_SLOT: &str = "statica/slot";
pub(crate) const META_PREFIX: &str = "statica:";
pub const DATA_T: &str = "data-t";
pub const DATA_T_ATTR_PREFIX: &str = "data-t-";
pub(crate) const DATA_BIND: &str = "data-bind";
pub(crate) const DATA_EACH: &str = "data-each";
pub(crate) const STATICA_FORM: &str = "statica";
pub(crate) const DATA_SCOPE: &str = "data-s";
pub(crate) const DATA_SCRIPT_SCOPE: &str = "data-s-scope";
pub(crate) const DATA_IMAGE: &str = "data-s-img";
pub(crate) const DATA_IMAGE_SIZES: &str = "data-s-img-sizes";
pub(crate) const ATTR_CLASS: &str = "class";
pub(crate) const SEARCH_RUNTIME_DIR: &str = "statica";
pub(crate) const SEARCH_JS_PATH: &str = "/statica/search.js";
pub(crate) const SEARCH_CSS_PATH: &str = "/statica/search.css";
pub(crate) const DATA_STATICA_SEARCH: &str = "data-statica-search";
pub(crate) const DATA_STATICA_SEARCH_CLOSE: &str = "data-statica-search-close";
pub(crate) const DATA_STATICA_SEARCH_META: &str = "data-statica-search-meta";
pub(crate) const DATA_STATICA_SEARCH_RESULTS: &str = "data-statica-search-results";
pub(crate) const FORWARDED_CLASS_SUFFIX: char = '+';

#[must_use]
pub(crate) fn rel_double_quoted(rel: &str) -> String {
    format!("rel=\"{rel}\"")
}

#[must_use]
pub(crate) fn rel_single_quoted(rel: &str) -> String {
    format!("rel='{rel}'")
}

#[must_use]
pub(crate) fn rel_double_quoted_data() -> String {
    rel_double_quoted(REL_DATA)
}

#[must_use]
pub(crate) fn missing_data_source_message(id: &str) -> String {
    format!(
        "no data source named `{id}` is available here; add <link rel=\"{REL_DATA}\" href=\"...\" id=\"{id}\"> to this page or fragment, or update the binding to use an existing data id"
    )
}

#[must_use]
pub(crate) fn missing_fragment_message(id: &str) -> String {
    format!(
        "no fragment named `{id}` has been imported; add <link rel=\"{REL_FRAGMENT}\" type=\"text/html\" href=\"...\" id=\"{id}\"> before mounting <slot id=\"{id}\">"
    )
}

#[must_use]
pub(crate) fn contains_forwarded_class_marker(value: &str) -> bool {
    value.contains(FORWARDED_CLASS_SUFFIX)
}

#[must_use]
pub(crate) fn forwarded_class_base(token: &str) -> Option<&str> {
    if token.len() == FORWARDED_CLASS_SUFFIX.len_utf8() && token.starts_with(FORWARDED_CLASS_SUFFIX)
    {
        return Some("");
    }
    token
        .strip_suffix(FORWARDED_CLASS_SUFFIX)
        .filter(|base| !base.is_empty())
}
