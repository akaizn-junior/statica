# statica

**Just HTML.** A blazingly fast static site generator for valid HTML.

Full reference: [docs/guide.md](docs/guide.md)

## Install

**Homebrew:**

```bash
brew tap akaizn-junior/statica
brew install statica
```

See [homebrew/README.md](homebrew/README.md) for tap setup.

**JavaScript (npm):**

```bash
npm i -D @statica/cli
npx statica
```

Create a new site directly from npm:

```bash
npm create statica@latest my-site
cd my-site
statica
```

**Rust (crates.io):**

```bash
cargo install statica-cli --locked
```

From this repo (dev):

```bash
cargo install --path crates/statica-cli --force
```

## Quick start

`statica new` creates a small localized starter with a shared layout, i18n catalogs, the statica badge, and copyable valid HTML.

```bash
npm create statica@latest my-site
cd my-site
statica                 # build, watch, and serve cwd
```

## Project layout

statica routes are folders, content is linked at build time, layouts hold shared page shells, and fragments hold reusable components.

```text
my-site/
├── statica.toml
├── index.html
├── 404/index.html
├── content/
│   ├── posts/
│   └── i18n/en.json
├── layouts/
│   └── base.html
├── ui/
│   └── post-card.html
├── posts/[slug]/index.html
├── blog/[page]/index.html
└── public/
    └── logo.svg
```

`public/`, `assets/`, and `static/` are copied by default through `asset_dirs`.

New projects keep reusable page chrome in `layouts/base.html`, route-specific content in `**/index.html`, reusable component templates in `ui/`, and funnel data in `content/`.

## CLI

```text
statica [PATH]              build + watch + serve (default)
statica build [PATH]        one-off build
statica serve [PATH]        preview latest build
statica watch [PATH]        watch mode
statica new <NAME>          scaffold
statica -h / --help
statica -v / --version
```

### Options

```bash
statica build --rss 'title=Blog,limit=20,collections=posts'
statica build --sitemap 'filename=sitemap.xml,urls_per_file=50000'
statica build --process 'css=true,js=false,images=true'
statica build --minify 'html=true,css=true,js=true'
statica build --process --minify
statica build --search 'output=assets/search.json'
statica build --pagination 'page_size=10,sort_desc=true,index=true'
statica build --i18n 'locales=en|pt,default=en'
statica build --render-mode serial
statica build --report-json report.json
statica watch --preview host=127.0.0.1,port=9000
```

## Config (`statica.toml`)

Optional. Missing file → defaults. See [docs/guide.md](docs/guide.md) for the full reference.

```toml
project = ""                 # relative to this file; empty = here
out_dir = ".website"
asset_dirs = ["public", "assets", "static"]
site_url = ""                # needed for sitemap / RSS
manifest = false

[aliases]
symbol = "@"

[aliases.urls]
Google = "https://fonts.googleapis.com/css2"

[aliases.paths]
static = "./static"
ui = "./ui"

[process]
enabled = false
css = true
js = true
images = true
fonts = false

[process.image]
widths = [480, 768, 1024, 1366, 1920]
formats = ["webp"]
quality = 85
sizes = "100vw"
responsive = true

[minify]
enabled = false
html = true
css = true
js = true

[sitemap]
enabled = false
urls_per_file = 50000

[[pagination]]
page_size = 10
sort_by = "published_at"
sort_desc = true
index = true

[rss]
enabled = false
limit = 50

[search]
enabled = false
output = "search.json"

[performance]
render_mode = "auto"
render_threads = 0

[preview]
host = "0.0.0.0"
port = 4321

[forms]
enabled = false
provider = "formspree"

[i18n]
enabled = false
locales = ["en"]
```

| Asset kind | Tool |
| ---------- | ---- |
| CSS | lightningcss (nesting, modern syntax → browser-ready; minify with `--process` or `--minify`) |
| JS | oxc |
| HTML | minify-html (final pass with `--minify`) |
| Images | oxipng + image |
| Fonts | copied as-is |

Inline `<style>` (pages + fragments) is always transformed. Linked `.css` under `asset_dirs` is transformed when `[process].css` is on. Enable `[minify]` / `--minify` for a final pass on emitted HTML, CSS, and linked JS; inline scripts are preserved so scoped fragment behavior stays exact.

Set `[performance].render_mode` to `auto`, `serial`, or `parallel`. `serial` avoids rayon for page rendering; `parallel` always uses rayon; `auto` uses statica's default page-render profile. Use `render_threads = 0` for the default worker count, or set `--render-threads N` to cap parallel page rendering.

Use `--report-json [PATH]` to write the build report as JSON for benchmarks, CI, and integrations. Omit `PATH` or pass `-` to write JSON to stdout; pass a file path to update that file. In `watch`, the report is written after the initial build and each rebuild.

`statica watch` performs conservative incremental rebuilds. Direct edits to an existing page `index.html` re-emit only that page route when global post-processing is off. Changes to shared inputs such as data, fragments, assets, config-driven processing, deleted files, or minified builds fall back to a full rebuild.

## Authoring

statica source is valid HTML. It uses normal `<template>`, `<slot>`, and `<link>` elements as build-time authoring primitives, so keep them where HTML allows them.

### Pages and routes

Every `index.html` is a page. Folder names become routes, and bracket folders expand from build-time data.

```text
index.html                 → .website/index.html
404/index.html             → .website/404/index.html
posts/[slug]/index.html    → .website/posts/{item.slug}/index.html
blog/[page]/index.html     → .website/blog/1/, blog/2/, …  ([[pagination]])
[locale]/about/index.html  → .website/en/about/, .website/pt/about/  ([i18n])
```

Static pages emit once. Collection pages use a bracket param such as `[slug]` and a linked data array; the current record is `item`. Pagination pages use `[page]` plus `[[pagination]]`; page metadata and items live under `page.pagination`.

### 404

If the site does not define `404.html` or `404/index.html`, statica writes a default `.website/404/index.html`. Custom 404 pages are normal source pages and always win. `statica serve` returns the 404 page with HTTP status `404` for missing paths.

### Data funnels

Data funnels load content at build time with `<link rel="statica/data">`. `href` points to a file or explicit glob, and `id` names the data in the page or fragment scope.

```html
<link rel="statica/data" href="content/posts/*.md" id="posts" />
<link rel="statica/data" href="content/vehicles.csv" id="vehicles" />
<link rel="statica/data" href="content/notes.txt" id="notes" type="text/plain" />
```

Supported sources are JSON, JSONL/NDJSON, CSV, plain text, Markdown, and globs of those files. Data is loaded during the build; production pages should not fetch site content at runtime.

Dynamic data `href` values use the same scoped attribute rules, so locale data can use paths like `href="../content/posts.${i18n.locale}.json"` after binding `{i18n}`.

### Binding basics

Use `data-bind` to declare scope, `data-t` to replace text, and `${...}` inside attributes.

```html
<html lang="en" data-bind="{item}">
  <head>
    <title data-t="${item.headline}">Post</title>
  </head>
  <body>
    <h1 data-t="${item.headline}">Post</h1>
    <a href="/posts/${item.slug}/">Read</a>
  </body>
</html>
```

- Scalar page text → `data-t="${item.field}"`, or literal text with `data-t="Plain text"`
- Attributes → `${item.slug}` / `${page.pagination.next_href}` / `${i18n.locale}`
- Page `data-bind` declares canonical roots such as `{item}`, `{page}`, `{data}`, or `{i18n}` before use
- Data link IDs are directly available by `id`; they cannot be named `data`, `item`, `page`, or `i18n`
- Placeholders must be dotted identifier paths; statica does not evaluate JavaScript expressions

### Fragments

Fragments are build-time HTML components. Import a fragment file, mount it with a matching `<slot id>`, and define a `<template>` with the same `id`.

```html
<!-- page -->
<link rel="statica/data" href="content/posts/*.md" id="posts" />
<link rel="statica/fragment" type="text/html" href="ui/post-card.html" id="post-card" />
<slot id="post-card" data-each="posts"></slot>
```

```html
<!-- ui/post-card.html -->
<template id="post-card" data-bind="{slug, headline}">
  <article>
    <h2 data-t="${headline}">Post</h2>
    <a href="/posts/${slug}/">Read</a>
  </article>
</template>
```

Fragment mounts pass the current context. `data-each` loops over an array and passes each item. Fragments never receive canonical page context automatically; pass values through the mount context or link fragment-local data.

Fragment scripts are scoped by default. Inside a fragment `<script>`, `document.querySelector`, `document.querySelectorAll`, and `document.getElementById` search only that fragment instance.

### Layouts

Layouts are build-time document shells. A page declares one layout with `<link rel="statica/layout">`; statica loads that layout, projects page content into layout slots, then continues normal data, fragment, binding, asset, and minify steps.

```html
<!-- layouts/base.html -->
<html lang="en">
  <head>
    <slot name="head"></slot>
  </head>
  <body>
    <header><slot name="nav">Fallback nav</slot></header>
    <main><slot></slot></main>
  </body>
</html>
```

```html
<!-- index.html -->
<html lang="en">
  <head>
    <link rel="statica/layout" href="layouts/base.html" />
    <title>Home</title>
  </head>
  <body>
    <nav slot="nav"><a href="/">Home</a></nav>
    <h1>Hello</h1>
  </body>
</html>
```

Page `<head>` children project into `<slot name="head">`. Page body children without `slot` project into the default layout slot. Body elements with `slot="name"` project into matching named slots; `<template slot="name">` projects its children without keeping the template wrapper.

The generated starter and `examples/blog` use this shape: `layouts/base.html` owns shared metadata, global styles, navigation, search, and footer; route pages import it and keep only page-specific head entries and body content.

### Aliases

Aliases allow short prefixes instead of repeating long local paths or URLs. They are configured in `statica.toml`, and the default leading symbol is `@`.

Use aliases anywhere statica resolves authoring paths, such as fonts, scripts, styles, fragments, data funnels, and assets.

```html
<link rel="statica/font" href="@Google/?family=Outfit&display=swap" />
<script type="module" src="@static/app.js"></script>
<link rel="statica/fragment" href="@ui/post-card.html" id="post-card" />
```

`[aliases.urls]` entries resolve to absolute URLs. `[aliases.paths]` entries resolve to local paths relative to `statica.toml`. The text after the alias name is preserved as the tail, so `@static/app.js` resolves against the `static` alias base.

### CSS, JS, images, and assets

Inline `<style>` in pages and fragments is always transformed with lightningcss. Linked `.css` under `asset_dirs` is transformed when `[process].css` is enabled.

```html
<link rel="stylesheet" href="/app.css" />
<script type="module" src="/app.js"></script>
<img src="/logo.svg" alt="statica" />
```

When `[process].enabled` and `[process].images` are on, statica optimizes copied raster images, writes responsive width variants, adds configured formats such as WebP, and rewrites local `<img>` tags to responsive `<picture>` markup when `[process.image].responsive` is true. Use `[process.image]` to control widths, formats, JPEG quality, and the default `sizes` value.

Use `<link rel="statica/font">` for font stylesheets. Google Fonts URLs get the expected preconnect hints once per page.

### Search

Add a generated browser-side search modal with one authoring input.

```html
<input type="statica/search" placeholder="Search" />
```

statica emits `/search.json` and small runtime files under `/statica/`. Configure the index with `[search]`, or use `--search 'output=assets/search.json'` from the CLI.

### Forms

Mark static forms with `statica`, then configure a provider endpoint. Formspree is the default provider.

```html
<form statica name="contact" method="post">
  <input name="email" type="email" required />
  <textarea name="message" required></textarea>
  <button type="submit">Send</button>
</form>
```

```toml
[forms]
enabled = true
provider = "formspree"
endpoint = "https://formspree.io/f/{id}"

[forms.ids]
contact = "your-form-id"
```

### i18n

Use a `[locale]` route segment and enable `[i18n]`. Catalogs live at `content/i18n/{locale}.json` by default.

```toml
[i18n]
enabled = true
locales = ["en", "pt"]
default = "en"
```

```html
<html lang="en" data-bind="{i18n}">
  <span data-t="${i18n.nav.home}">Home</span>
  <a href="/${i18n.locale}/">Home</a>
</html>
```

Pages must bind `{i18n}` before using catalog values. Fragments do not receive `i18n` automatically.

## Deploy

`statica build` writes plain static files to `.website/` by default. Deploy that output directory to any static host.

```bash
statica build
```

## License

MIT

## Author

(c) 2026 Simão Nziaka
