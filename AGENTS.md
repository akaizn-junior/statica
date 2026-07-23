# AGENTS.md — statica

Instructions for AI coding agents working in this repository.

> Always lowercase **statica** (product name, CLI, crate names in prose).

## What this is

**statica** is a static site generator: **Powered HTML**. Authors write valid HTML; the build resolves fragments, runs build-time data funnels, expands collections and pagination, scopes component CSS/JS, and emits static files.

| Concept | Role |
| ------- | ---- |
| **Funnel** | Build-time data via `<script type="statica/data" src id>` |
| **Pages** | Every `**/index.html` — folder path is the route (`[slug]`, `[page]`, `[locale]`) |

Flow: **Funnel → Pages → static HTML** (default output: `.dist/`)

This repo contains two things:

1. **The statica engine** — Rust workspace (`crates/statica`, `crates/statica-cli`)
2. **Example sites** — `examples/blog` (dogfood fixture), bench fixtures

Do not treat statica like React, Vue, or Next.js. There is no client-side framework, no JSX, no virtual DOM. HTML + statica attributes **are** the template language.

## Quick commands

```bash
# Build CLI (also regenerates man pages in docs/man/)
cargo build -p statica-cli --release

# Run tests
cargo test -p statica
cargo test -p statica-cli

# Build the dogfood site
statica build examples/blog

# Dev loop (from a site directory)
statica watch

# Prefer installed binary over cargo run
statica build .
```

CI runs `cargo build -p statica-cli --release` and `cargo test` on push/PR. Pushing a version bump to `main` tags `v{version}` and dispatches the global release build (binaries, GitHub Release, crates.io, npm).

## Documentation map

| Need | Read |
| ---- | ---- |
| Full authoring + config reference | [docs/guide.md](docs/guide.md) |
| Human overview + install | [README.md](README.md) |
| Working example site | [examples/blog/](examples/blog/) |
| Pipeline architecture | [crates/statica/src/lib.rs](crates/statica/src/lib.rs) |
| All config options | [crates/statica-cli/src/config.rs](crates/statica-cli/src/config.rs) |
| Expected build behavior | [crates/statica/tests/build_fixture.rs](crates/statica/tests/build_fixture.rs) |
| Rust core conventions | [crates/statica/AGENTS.md](crates/statica/AGENTS.md) |
| CLI crate conventions | [crates/statica-cli/AGENTS.md](crates/statica-cli/AGENTS.md) |
| Site authoring in examples | [examples/blog/AGENTS.md](examples/blog/AGENTS.md) |

---

## Writing statica sites (the statica way)

When creating or editing HTML sites that statica builds — whether in `examples/`, scaffolds from `statica new`, or user projects — follow these rules.

### Mental model

- **Routing is filesystem-based.** `about/index.html` → `/about/`. Dynamic segments use bracket folders: `posts/[slug]/index.html`.
- **Data is build-time only.** Funnel scripts load JSON, JS value literals, or Markdown directories at build time. Production output is plain static HTML — no runtime data fetching.
- **Fragments are HTML components.** `<template id="…">` in a fragment file, imported with `<link rel="statica/fragment">`, mounted with `<slot id="…">`.

### The three-part fragment contract

Every fragment needs matching `id` on all three parts:

```html
<!-- 1. Import -->
<link rel="statica/fragment" type="text/html" href="../ui/post-card.html" id="post-card" />

<!-- 2. Mount -->
<slot id="post-card" data-bind="."></slot>

<!-- 3. Template (in ui/post-card.html) -->
<template id="post-card" data-bind="{slug, headline}">
  <a href="/posts/${slug}/"><slot name="headline"></slot></a>
</template>
```

### Binding rules (strict — build fails if violated)

| Use case | Syntax | Notes |
| -------- | ------ | ----- |
| Text content | `<slot name="field">` | Never put `<slot>` inside attributes |
| Attributes | `${field}` | Field must be declared in `data-bind` |
| Whole object | `data-bind="posts"` on fragments | Use `${posts.field}` or nested slots |
| Page collection | `data-bind="posts"` or `data-bind="{headline, …}"` | Named → `${posts.field}`; destructure → `<slot name="field">` |
| Destructured fields | `data-bind="{slug, headline}"` | Use `${slug}` directly |
| Current item in loop | `data-bind="."` | On `<slot>` inside `data-each` |
| Loops | `data-each="items"` | On `<slot id="fragment-id">` |

**No magic flattening.** If you write `${variant}`, you must bind `{variant, …}` or `${button.variant}` with `data-bind="button"`. Wrong bindings produce `file:line:column` diagnostics.

### Page types

**Static page** — plain `index.html`, one output.

**Collection page** — `[param]` folder + root `<html data-bind="arrayId">` or `data-bind="{fields…}">`:

```html
<html lang="en" data-bind="{headline, summary}">
  <script type="statica/data" src="../../content/posts" id="posts"></script>
  <title><slot name="headline"></slot></title>
</html>
```

**Pagination page** — `[page]` folder + `[[pagination]]` in `statica.toml`:

```html
<html lang="en" data-bind="{page, total_pages, items, prev_href, next_href, pages}">
  <slot id="post-list" data-bind="items"></slot>
  <a href="${prev_href}">Previous</a>
</html>
```

Page context includes: `items`, `page`, `total_pages`, `prev_href`, `next_href`, `pages`, etc. See [docs/guide.md](docs/guide.md).

**i18n page** — `[locale]/` segment + `[i18n]` config. Use `data-t="key"` for translatable text; `${locale}` in attributes only (not text nodes).

### CSS and JS in fragments

- Write modern CSS in `<style>` (nesting, `@media (width >= 40rem)`, etc.). statica compiles with lightningcss; fragment styles are scoped via `[data-s="id-hash"]`.
- Fragment `<script type="module">` uses `$` to scope DOM queries to the fragment instance. Production builds inline the helper.

### Paths and aliases

- Funnel `src`, fragment `href`, and asset paths are **relative to the HTML file** that declares them.
- Aliases in `statica.toml` use `@Name/tail` syntax (e.g. `@Google/?family=…`, `@static/app.js`).

### Site layout convention

```
my-site/
├── statica.toml
├── index.html
├── content/           # funnel sources (JSON, JS, Markdown)
│   └── i18n/{locale}.json
├── ui/                # fragment templates
├── posts/[slug]/index.html
├── blog/[page]/index.html
└── public/            # static assets (copied to out_dir)
```

### Authoring anti-patterns

Do **not**:

- Introduce React/Vue/Svelte components or a bundler-centric workflow unless explicitly requested
- Use `${field}` in text nodes — use `<slot name="field">` instead
- Put `<slot>` inside HTML attributes
- Mix `[page]` and `[slug]` under the same route tree (e.g. both `posts/[slug]` and `posts/[page]`)
- Assume undeclared fields bind automatically — every `${…}` and named slot must appear in `data-bind`
- Use runtime fetch/API calls for content that should be static
- Capitalize "Statica" in user-facing copy — always **statica**

Do:

- Copy patterns from [examples/blog/](examples/blog/) before inventing new structure
- Keep fragments in `ui/`, content in `content/`, routes as `**/index.html`
- Run `statica build` (or `statica watch`) to verify changes
- Use `<form statica>` + `[forms]` config for forms — no client JS injection

---

## Contributing to the Rust engine

When editing `crates/statica` or `crates/statica-cli`, read the nested AGENTS.md in that crate.

### Architecture boundary (critical)

| Crate | Owns |
| ----- | ---- |
| `statica-cli` | `statica.toml`, env files, CLI flags, watch/serve/scaffold, man pages |
| `statica` | Pipeline: discover → pre → parse → funnel → expand → bind → scope → emit |

**Core never reads config files.** The CLI maps TOML + flags → `BuildOptions` and calls `statica::build(&opts)`.

### Error handling

- **Core:** typed `statica::Error` with `Diagnostic` for authoring mistakes (`file:line:column` + snippet)
- **CLI:** `anyhow::Result` with `.context()` at the boundary

### Testing

- Prefer integration tests calling `build(&opts)` and asserting on emitted HTML strings
- Use `examples/blog` as the canonical fixture; see `build_fixture.rs`
- Co-locate unit tests in `mod tests { }` blocks inside source files
- Behavior-focused: assert output, not internal AST state

### Rust style

- Edition 2021, standard rustfmt defaults
- Clippy pedantic enabled in core with targeted allows (see `lib.rs`)
- Module-level `//!` docs on every major module
- `#[must_use]` on constructors and builders
- Serde config: `#[serde(default, deny_unknown_fields)]`

### Pipeline stages (do not reorder casually)

1. discover → 2. pre → 3. parse → 4. funnel → 5. expand → 6. bind → 7. scope → 8. emit → 9. minify

Authoring HTML is parsed with **html5ever**, not regex.

### Adding features

1. Read [docs/guide.md](docs/guide.md) and `examples/blog` — dogfood new authoring features there first
2. Add integration test in `build_fixture.rs` for end-to-end behavior
3. Add unit tests for edge cases in the relevant module
4. Update guide + README if user-facing
5. Man pages regenerate automatically on `cargo build -p statica-cli` — update clap help text in `cli.rs` if CLI changed

---

## Boundaries

- Do not commit secrets (`.env`, API keys, Formspree IDs in examples are placeholders)
- Do not force-push to `main`
- Do not add framework dependencies to example sites unless explicitly requested
- Minimize scope — match existing patterns before introducing abstractions
- Only create git commits when the user asks

## License

MIT
