# AGENTS.md — statica

Instructions for AI coding agents working in this repository.

> Always lowercase **statica** (product name, CLI, crate names in prose).

## What this is

**statica** is **Just HTML.** A blazingly fast static site generator that builds on just HTML Authors write valid HTML files. statica reads those files at build time, loads declared data, expands pages, resolves fragments, binds values into attributes/text, scopes fragment CSS/JS, and emits plain static files.

| Concept | Role |
| ------- | ---- |
| **Pages** | Every `**/index.html`; the folder path is the route |
| **Routes** | Static folders plus bracket params like `[slug]`, `[page]`, and `[locale]` |
| **Funnels** | Build-time data linked with `<link rel="statica/data" href="..." id="...">` |
| **Fragments** | Build-time HTML components imported with `<link rel="statica/fragment" ...>` |
| **Binding** | Static replacement through `data-bind`, `data-t`, attributes, slots, and `data-each` |

Pipeline: **discover → pre → parse → funnel → expand → bind → scope → emit → minify** (default output: `.website/`)

This repo contains two things:

1. **The statica engine** — Rust workspace (`crates/statica`, `crates/statica-cli`)
2. **Example sites** — `examples/blog` (dogfood fixture), bench fixtures

Do not treat statica like React, Vue, Svelte, Astro, or Next.js. There is no client-side component runtime, no JSX, no virtual DOM, and no runtime content fetch for site data. HTML + statica attributes **are** the template language.

## Quick commands

```bash
# Build CLI (also regenerates man pages in docs/man/)
cargo build -p statica-cli --release

# Run tests
cargo test -p statica
cargo test -p statica-cli

# Build the dogfood site (default command is build)
statica examples/blog

# Dev loop (watch + serve)
statica watch

# Prefer installed binary over cargo run; `statica` builds cwd
statica
```

CI runs `cargo build -p statica-cli --release` and `cargo test` on push/PR. Pushing a version bump to `main` tags `v{version}` and dispatches the global release build (binaries, GitHub Release, crates.io, npm, Homebrew tap).

## Change checklist

- Engine behavior: update tests in `crates/statica`, then `docs/guide.md`; update `examples/blog` when the feature needs a dogfood example.
- CLI behavior: update clap help, `README.md`, `docs/guide.md`, and regenerate `docs/man/` with `cargo build -p statica-cli --release`.
- Site authoring examples: keep `examples/blog` canonical and mention new patterns in `examples/blog/AGENTS.md` when future agents should copy them.
- User-facing copy: spell the product, binary, and crate names as lowercase `statica`.

## Documentation map

| Need | Read |
| ---- | ---- |
| Direct authoring + config reference | [docs/guide.md](docs/guide.md) |
| Install + new-site flow | [README.md](README.md) |
| Working example site | [examples/blog/](examples/blog/) |
| Pipeline architecture | [crates/statica/src/lib.rs](crates/statica/src/lib.rs) |
| All config options | [crates/statica-cli/src/config.rs](crates/statica-cli/src/config.rs) |
| Expected build behavior | [crates/statica/tests/build_fixture.rs](crates/statica/tests/build_fixture.rs) |
| Rust core conventions | [crates/statica/AGENTS.md](crates/statica/AGENTS.md) |
| CLI crate conventions | [crates/statica-cli/AGENTS.md](crates/statica-cli/AGENTS.md) |
| Site authoring in examples | [examples/blog/AGENTS.md](examples/blog/AGENTS.md) |

---

## CLI flow

- `statica [PATH]` is the default build command; `statica build [PATH]` is equivalent.
- `statica watch [PATH]` rebuilds and serves; `statica serve [PATH]` previews `out_dir`; `statica new <NAME>` scaffolds.
- Resolve `PATH` against the process **cwd**, then walk up for `statica.toml`.
- The site root is the config directory, or `project` / `--project` under it.
- CLI SPEC strings override TOML for nested config (`--rss`, `--sitemap`, `--process`, `--minify`, `--pagination`, `--i18n`, `--preview`); scalar flags like `--render-mode` and `--render-threads` override their matching TOML keys.
- When changing CLI behavior, update clap help, docs/guide.md, README.md, and regenerate man pages with `cargo build -p statica-cli --release`.

## Writing statica sites (the statica way)

When creating or editing HTML sites that statica builds — whether in `examples/`, scaffolds from `statica new`, or user projects — follow these rules.

### Non-negotiable model

- **Source must be valid HTML.** statica uses normal `<html>`, `<head>`, `<body>`, `<link>`, `<template>`, and `<slot>` elements as build-time authoring primitives. Do not put elements where HTML forbids them, such as `<slot>` inside `<title>` or inside attributes.
- **Routing is filesystem-based.** `about/index.html` emits `/about/`. Dynamic segments use bracket folders: `posts/[slug]/index.html`, `blog/[page]/index.html`, `[locale]/about/index.html`.
- **404 is static too.** Authors may define `404/index.html` or `404.html`; if they do not, statica emits a default `404/index.html`. Preview serving returns missing paths with HTTP 404 using that page.
- **Data is linked, not guessed.** Funnel data comes from `<link rel="statica/data" href="..." id="...">`; `href` must point to a file or explicit glob, never a directory.
- **Data is build-time only.** Production output is static HTML/CSS/JS. Do not add runtime fetches for content that statica should funnel.
- **Fragments are build-time components.** A fragment is a `<template id="...">` imported and mounted by matching `id`. Fragment CSS/JS is scoped at build time.
- **Context is explicit.** Pages may use canonical roots only after `<html data-bind="...">` asks for them. Fragments never receive canonical page context.
- **Default build command is short.** Prefer examples like `statica .` or `statica examples/blog`; use `statica build …` when documenting the explicit subcommand.
- **Page rendering mode is configurable.** `[performance].render_mode` / `--render-mode` controls page rendering (`auto`, `serial`, `parallel`); `[performance].render_threads` / `--render-threads` caps parallel page-render workers (`0` means auto).
- **Verification follows the layer changed.** Core changes need core tests; CLI changes need CLI tests and regenerated man pages; authoring changes should build the fixture.

### Data funnels

Use `<link rel="statica/data">` in the page or fragment file that needs the data.

```html
<link rel="statica/data" href="../../content/posts/*.md" id="posts" />
<link rel="statica/data" href="../content/vehicles.csv" id="vehicles" />
<link rel="statica/data" href="../content/notes.txt" id="notes" type="text/plain" />
```

Rules:

- `href` is relative to the HTML file declaring the link, after aliases are resolved.
- `href` must be a concrete file or an explicit glob such as `posts/*.md`; `posts/` is invalid.
- Supported sources are JSON, JSONL/NDJSON, CSV, plain text, Markdown, and globs of those files.
- Plain text becomes an array of non-empty strings. JSONL/NDJSON becomes an array of parsed line values. CSV becomes an array of objects keyed by header.
- Data link `id` names are available by id in that page/fragment scope.
- Data link `id` names cannot be `data`, `item`, `page`, or `i18n`; those are canonical page roots.
- Dynamic data `href` values use the same scoped attribute rules as other `${...}` attributes and expand before file/glob loading. Locale-specific data uses canonical `i18n.locale`, e.g. `href="../../content/posts.${i18n.locale}.json"`.

### Page context

statica builds a canonical page object:

```json
{
  "data": {},
  "item": null,
  "page": {
    "route": "",
    "params": {}
  },
  "i18n": {
    "locale": ""
  }
}
```

Canonical roots:

| Root | Meaning |
| ---- | ------- |
| `data` | All linked page data keyed by data link `id` |
| `item` | Current collection record for `[param]` routes |
| `page.route` | Filesystem route for the current page |
| `page.params` | Route params resolved for the emitted page |
| `page.pagination` | Pagination chunk metadata for `[page]` routes |
| `i18n.locale` | Active locale when i18n is enabled |

Pages must opt into canonical roots with `<html data-bind>`.

```html
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content/posts/*.md" id="posts" />
    <title data-t="${item.headline}">Post</title>
  </head>
  <body>
    <h1 data-t="${item.headline}">Post</h1>
    <slot name="item.html"></slot>
  </body>
</html>
```

Page lookup order is:

1. Bound page data from `<html data-bind="...">`
2. Linked data ids from valid `statica/data` links
3. No fallback

If a page uses `${item.slug}`, `data-t="${page.pagination.page}"`, `data-each="item.related"`, or `data-t="${i18n.about.title}"`, then the page must bind `{item}`, `{page}`, or `{i18n}` on `<html>`.

### Binding syntax

Use `data-bind` to define what names exist. Use `data-t` for text. Use `${...}` only inside attributes and `data-t`.

```html
<!-- Destructure specific fields -->
<template id="post-card" data-bind="{slug, headline}">
  <a href="/posts/${slug}/" data-t="${headline}">Post</a>
</template>

<!-- Bind the whole object -->
<template id="post-card" data-bind="post">
  <a href="/posts/${post.slug}/" data-t="${post.headline}">Post</a>
</template>
```

Rules:

- `data-bind` is valid only on page `<html>` and fragment `<template>`.
- `data-t="${path.to.value}"` replaces element text.
- Literal `data-t="Plain text"` renders that literal text.
- `${path.to.value}` works in attributes.
- Dynamic placeholders in attributes are generic; validate them by scope, not by special-casing variable names such as `locale`.
- `${...}` is never valid directly in text nodes.
- Placeholders must be dotted identifier paths, not JavaScript expressions.
- statica does not evaluate `${a + b}`, function calls, filters, optional chaining, array indexing, or arbitrary JS.
- There is no magic flattening. `${headline}` works only if `headline` is actually bound; otherwise use `${item.headline}` and bind `{item}`.

### The three-part fragment contract

Every fragment needs matching `id` on all three parts:

```html
<!-- 1. Import -->
<link rel="statica/fragment" type="text/html" href="../ui/post-card.html" id="post-card" />

<!-- 2. Mount -->
<slot id="post-card"></slot>

<!-- 3. Template (in ui/post-card.html) -->
<template id="post-card" data-bind="{slug, headline}">
  <a href="/posts/${slug}/" data-t="${headline}">Post</a>
</template>
```

### Fragment context

Fragments do not see canonical page context. A fragment can use only:

1. Values declared by its own `<template data-bind="...">`
2. Data linked inside that fragment file with `<link rel="statica/data" ...>`

Fragment lookup order is:

1. Bound fragment data
2. Linked fragment data ids
3. No fallback

Fragment mounts pass the current render value by default. In a loop, each array item is the value passed to the fragment.

```html
<template id="post-list" data-bind="{items}">
  <ul>
    <slot id="post-card" data-each="items"></slot>
  </ul>
</template>
```

`data-each` is valid on fragment mount slots: `<slot id="fragment-id" data-each="items"></slot>`. `data-bind` is not valid on mount slots.

### Page types

**Static page** — plain `index.html`, one output.

**Collection page** — `[param]` folder + linked array data source:

```html
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content/posts/*.md" id="posts" />
    <title data-t="${item.headline}">Post</title>
  </head>
</html>
```

**Pagination page** — `[[pagination]]` config + `[page]` folder:

```html
<html lang="en" data-bind="{page}">
  <slot id="post-list"></slot>
  <a href="${page.pagination.prev_href}">Previous</a>
</html>
```

Page context roots are `data`, `item`, `page`, and `i18n`; pagination metadata lives at `page.pagination`. Pages must declare canonical roots with `<html data-bind="…">` before use. Data link ids are available by id, but cannot collide with canonical roots. See [docs/guide.md](docs/guide.md).

Paginated roots may also contain nested item pages:

```text
# statica.toml: [[pagination]]
blog/[page]/index.html
blog/[page]/[slug]/index.html
```

Nested item pages bind `{page, item}`. The `[page]` segment comes from pagination; `[slug]` comes from the item inside that page chunk.

**i18n page** — `[locale]/` segment + `[i18n]` config. Use `data-t="${i18n.section.key}"` for catalog text; `${i18n.locale}` works in attributes and `data-t`, not text nodes.

### CSS and JS in fragments

- Write modern CSS in `<style>` (nesting, `@media (width >= 40rem)`, etc.). statica compiles with lightningcss; fragment styles are scoped via `[data-s="id-hash"]`.
- Fragment `<script>` runs with scoped `document` selector methods by default. Inside a fragment script, `document.querySelector`, `document.querySelectorAll`, and `document.getElementById` search only the current fragment instance. Production builds inline the helper.
- Inline `<style>` in pages and fragments is always transformed.
- Linked `.css` under asset dirs is transformed only when `[process].css` / `--process css=true` is enabled.
- Final output minification is controlled by `[minify]` / `--minify` for emitted HTML, CSS, and JS, including inline `<style>` / `<script>`.

### Styling statica sites with daisyUI

When building new statica sites or substantially refreshing example/user-facing pages, prefer **daisyUI via HTML classes** as the default styling approach unless the existing site already has a clear custom design system.

- Use daisyUI's semantic component classes directly in statica HTML and fragments: `navbar`, `hero`, `card`, `btn`, `badge`, `alert`, `menu`, `tabs`, `steps`, `stats`, `timeline`, `collapse`, `modal`, `drawer`, `footer`, and form controls.
- Lean into daisyUI themes and utility composition to make pages beautiful quickly: choose an intentional `data-theme`, use responsive Tailwind/daisyUI classes, and refine spacing, hierarchy, contrast, and interaction states.
- Keep the site HTML-powered. Compose real HTML elements, statica fragments, slots, and `data-*` bindings; do not introduce React/Vue/Svelte or client-side component runtimes just to use daisyUI.
- Prefer rich, polished default screens over bare wireframes. Use daisyUI's built-in visual language for navigation, calls to action, content cards, metadata badges, pagination, forms, empty states, and status messages.
- Add only small custom CSS where daisyUI classes cannot express the design cleanly. Custom CSS should enhance the component system, not replace it wholesale.
- Make generated authoring examples copyable: valid HTML, accessible landmarks, labeled controls, responsive layouts, and statica-compatible `data-t`, `${...}`, `data-each`, and fragment usage.

### Forms

Prefer Formspree for statica forms unless the user or existing project specifies another backend.

- Use `<form statica>` with `[forms]` config for build-time form wiring; do not inject client-side JavaScript for normal submissions.
- Keep Formspree IDs, endpoints, and secrets out of committed examples unless they are explicit placeholders.
- Style forms with daisyUI form controls (`form-control`, `label`, `input`, `textarea`, `select`, `checkbox`, `radio`, `btn`) so contact, signup, survey, and feedback flows feel complete and polished.

### Asset pipeline

| Asset kind | Tool |
| ---------- | ---- |
| CSS | lightningcss |
| JS | oxc |
| HTML | minify-html |
| Images | oxipng + image |
| Fonts | copied as-is |

### Paths and aliases

- Funnel `href`, fragment `href`, and asset paths are **relative to the HTML file** that declares them.
- Aliases in `statica.toml` use `@Name/tail` syntax (e.g. `@Google/?family=…`, `@static/app.js`).

### Site layout convention

```
my-site/
├── statica.toml
├── index.html
├── content/           # funnel sources (JSON, JSONL, CSV, text, Markdown)
│   └── i18n/{locale}.json
├── ui/                # fragment templates
├── posts/[slug]/index.html
├── blog/[page]/index.html
└── public/            # static assets (copied to out_dir)
```

### Authoring anti-patterns

Do **not**:

- Introduce React/Vue/Svelte components or a bundler-centric workflow unless explicitly requested
- Use `${field}` directly in text nodes — use `data-t="${field}"` instead
- Put `<slot>` inside HTML attributes
- Put `<slot>` inside `<title>`; use `data-t` on `<title>` instead
- Put `data-bind` on fragment mount slots; use `data-each` on mounts and `data-bind` on fragment templates
- Mix `[page]` and `[slug]` under the same route tree (e.g. both `posts/[slug]` and `posts/[page]`)
- Assume undeclared fields bind automatically — every `${…}` in attributes or `data-t` must be a dotted identifier path bound by `data-bind`
- Use canonical roots in pages without `<html data-bind>` documenting them
- Use canonical roots in fragments at all; pass values through fragment `data-bind` or link fragment-local data
- Use runtime fetch/API calls for content that should be static
- Capitalize "Statica" in user-facing copy — always **statica**

Do:

- Copy patterns from [examples/blog/](examples/blog/) before inventing new structure
- Prefer daisyUI HTML classes for beautiful, polished statica sites when no existing design system says otherwise
- Keep fragments in `ui/`, content in `content/`, routes as `**/index.html`
- Run `statica build` (or `statica watch`) to verify changes
- Use `<form statica>` + `[forms]` config for forms, and prefer Formspree as the form backend

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
