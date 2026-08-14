//! Core statica authoring and runtime contract tokens.

pub(crate) const REL_DATA: &str = "statica/data";
pub(crate) const REL_FRAGMENT: &str = "statica/fragment";
pub(crate) const REL_FONT: &str = "statica/font";
pub(crate) const TYPE_SEARCH: &str = "statica/search";
pub(crate) const TYPE_SLOT: &str = "statica/slot";
pub(crate) const META_PREFIX: &str = "statica:";

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
