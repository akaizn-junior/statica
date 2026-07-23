# AGENTS.md — statica

Rust library: the statica build pipeline. Read [../../AGENTS.md](../../AGENTS.md) for project-wide context.

## Responsibility

`statica` transforms a site directory into static HTML. It receives [`BuildOptions`](src/build.rs) and returns a [`BuildReport`](src/build.rs). It **never reads `statica.toml`** — that is the CLI's job.

## Pipeline

Documented in [`src/lib.rs`](src/lib.rs):

```
discover → pre → parse → funnel → expand → bind → scope → emit → minify
```

| Module | Purpose |
| ------ | ------- |
| `discover` | Find `**/index.html`, detect `[param]` routes |
| `parse/` | pre → html5ever → normalize → owned AST |
| `funnel/` | Load `<script type="statica/data">` (JSON, JS literals, Markdown) |
| `bind/` | Slots, `${…}` attrs, `data-each`, fragments, i18n, forms |
| `scope/` | Hash-scoped CSS/JS for fragments |
| `emit` | Write HTML; CSS transform; asset copy/process |
| `feeds` | Sitemap + RSS |
| `paginate` | Chunk arrays into page objects |
| `i18n` | Locale expansion, `data-t` translation |

## Conventions

### Errors

Use `crate::error::Error` and `Diagnostic` for authoring problems. Diagnostics must include `file:line:column` and optional source snippets via [`loc.rs`](src/loc.rs).

```rust
// Authoring mistake → Diagnostic
// I/O or internal → Error::Io, etc.
```

The CLI maps these to `anyhow::Error` at the boundary.

### Public API

Re-export from `lib.rs`. Keep the surface minimal:

- `build`, `BuildOptions`, `BuildReport`
- Options structs for features (i18n, pagination, forms, minify, etc.)
- `Document` for parse results when needed externally

### Tests

- **Integration:** [`tests/build_fixture.rs`](tests/build_fixture.rs) — full pipeline against `examples/blog` or inline temp dirs
- **Unit:** `mod tests { }` co-located in source files
- Assert on **emitted HTML strings**, not internal state
- Temp dir helper pattern at bottom of `build_fixture.rs`

### Adding a pipeline feature

1. Identify the correct stage — binding logic belongs in `bind/`, not `emit`
2. Add unit tests in the module
3. Add integration test in `build_fixture.rs` with minimal HTML fixture
4. Update `docs/guide.md` if user-facing

### Dependencies

Key crates: html5ever, lightningcss, oxc, pulldown-cmark, rayon (parallel expansion), sitemap-rs, rss.

Do not add heavy dependencies without clear need. Prefer extending existing modules.

### Clippy

Pedantic warnings enabled in `lib.rs` with targeted allows. Match existing allow list when adding modules.
