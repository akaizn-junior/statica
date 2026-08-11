//! Scope fragment scripts using the embedded statica.js helper.

use crate::runtime::STATICA_JS;

/// Wrap author script so `document` is scoped to this fragment instance.
#[must_use]
pub fn wrap_script_with_scope(body: &str, scope_id: &str) -> String {
    format!(
        r#"{runtime}
__statica.run(document.currentScript, "{scope}", function (document) {{
  {body}
}});
"#,
        runtime = STATICA_JS,
        scope = scope_id,
        body = body.trim()
    )
}
