//! `${path}` templates in attribute values only (never slots-in-attrs).

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
    for (_k, v) in el.attrs.iter_mut() {
        if v.contains("${") {
            *v = expand_template(v, ctx);
        }
    }
}

fn expand_template(raw: &str, ctx: &Value) -> String {
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
    use serde_json::json;

    #[test]
    fn expands_slug_in_href() {
        let ctx = json!({"slug": "hello-world"});
        assert_eq!(
            expand_template("/posts/${slug}/", &ctx),
            "/posts/hello-world/"
        );
    }
}
