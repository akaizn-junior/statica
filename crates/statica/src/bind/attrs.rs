//! `${path}` templates in attribute values only (never slots-in-attrs).
//!
//! Static validation in `funnel::bind_decl` rejects non-path placeholders before
//! rendering. Runtime expansion stays lenient for already-validated templates.

use serde_json::Value;

use crate::funnel;
use crate::parse::{Element, Node};

pub fn fill_attr_templates_in_nodes(nodes: &mut [Node], ctx: &Value) {
    for node in nodes {
        if let Node::Element(el) = node {
            fill_attrs(el, ctx);
            fill_attr_templates_in_nodes(&mut el.children, ctx);
        }
    }
}

fn fill_attrs(el: &mut Element, ctx: &Value) {
    if el.is_script() || el.is_style() {
        return;
    }
    for (name, v) in &mut el.attrs {
        if is_data_t_marker(name) {
            continue;
        }
        if v.contains("${") {
            *v = expand_template(v, ctx);
        }
    }
}

fn is_data_t_marker(name: &str) -> bool {
    name == "data-t" || name.starts_with("data-t-")
}

pub(crate) fn expand_template(raw: &str, ctx: &Value) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = raw[i + 2..].find('}') {
                let path = raw[i + 2..i + 2 + end].trim();
                out.push_str(&funnel::path_as_str(ctx, path));
                i = i + 2 + end + 1;
                continue;
            }
        }
        out.push(raw[i..].chars().next().unwrap_or('\0'));
        i += raw[i..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::funnel::{bind_context, BindDecl};
    use serde_json::json;

    #[test]
    fn expands_slug_in_href() {
        let ctx = json!({"slug": "hello-world"});
        assert_eq!(
            expand_template("/posts/${slug}/", &ctx),
            "/posts/hello-world/"
        );
    }

    #[test]
    fn named_prop_no_magic_flatten() {
        let button = json!({"variant": "ghost", "href": "/x"});
        let ctx = bind_context(&BindDecl::Named("button".into()), &button);
        assert_eq!(expand_template("${button.href}", &ctx), "/x");
        assert_eq!(expand_template("${variant}", &ctx), ""); // not in context
    }

    #[test]
    fn destructure_exposes_listed_fields() {
        let ctx = bind_context(
            &BindDecl::destructure_flat(["variant", "href"]),
            &json!({"variant": "ghost", "href": "/x"}),
        );
        assert_eq!(
            expand_template(r#"class="button ${variant}" href="${href}""#, &ctx),
            r#"class="button ghost" href="/x""#
        );
    }

    #[test]
    fn template_expressions_are_not_evaluated() {
        let ctx = json!({"a": 1, "b": 2});
        assert_eq!(expand_template("${a + b}", &ctx), "");
    }
}
