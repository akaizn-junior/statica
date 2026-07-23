//! Inject `$` binding using the embedded statica.js helper.

use crate::runtime::STATICA_JS;

/// Wrap author script so `$` is bound to this fragment instance.
#[must_use]
pub fn wrap_script_with_scope(body: &str, scope_id: &str) -> String {
    // Prefer the exported factory when present; also define __staticaScope for one-shot inline.
    format!(
        r#"{runtime}
function __staticaScope(scriptEl, scopeId) {{
  return __statica.scope(scriptEl, scopeId);
}}
(function (scriptEl) {{
  const $ = __statica.scope(scriptEl, "{scope}");
  {body}
}})(document.currentScript);
"#,
        runtime = STATICA_JS,
        scope = scope_id,
        body = body.trim()
    )
}
