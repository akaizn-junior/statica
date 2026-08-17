# statica guide

**Just HTML.** A blazingly fast static site generator for valid HTML.

For install and starting a new site, use [../README.md](../README.md).

statica source files are valid HTML. Data, layouts, fragments, translations, forms, and pagination are resolved at build time; the generated site is plain static HTML/CSS/JS.

## Commands

```bash
statica [PATH]              # build + watch + serve (default)
statica build [PATH]        # one-off build
statica watch [PATH]        # watch + rebuild + serve
statica serve [PATH]        # serve out_dir with 404 fallback
statica new <NAME>          # scaffold
npm create statica@latest <NAME>
```

`PATH` defaults to `.`. statica resolves it against the process cwd, walks up for `statica.toml`, then uses that directory as the site root. If `project` or `--project` is set, the site root is that subdirectory under the config directory.

`statica new` creates a small localized starter site with a centered logo, Guide and GitHub links, and English/French/Portuguese catalogs under `content/i18n/`. The npm create flow runs the same scaffold.

Repository examples use the same visual baseline as the scaffold: real statica logo assets in `public/`, statica green accents, daisyUI-friendly controls, and valid HTML fragments/data links. Copy from `examples/blog` when you need a complete authoring reference.

## Project Layout

```text
my-site/
├── statica.toml
├── index.html
├── 404/index.html
├── content/
│   ├── posts/
│   └── i18n/en.json
├── ui/
│   └── post-card.html
├── posts/[slug]/index.html
├── blog/[page]/index.html
└── public/
    └── logo.svg
```

`public/`, `assets/`, and `static/` are copied by default through `asset_dirs`. Every `index.html` is a page. Folder path is the route.

```text
index.html                 → .website/index.html
about/index.html           → .website/about/index.html
404/index.html             → .website/404/index.html
posts/[slug]/index.html    → .website/posts/{item.slug}/index.html
blog/[page]/index.html     → .website/blog/1/, .website/blog/2/, ...
[locale]/about/index.html  → .website/en/about/, .website/pt/about/  ([i18n])
```

## Data

Load build-time data with `<link rel="statica/data">`. `href` points to a file or explicit glob, and `id` names the data in the page or fragment scope.

```html
<!-- index.html -->
<link rel="statica/data" href="content/posts.json" id="posts" />
<link rel="statica/data" href="content/posts/*.md" id="post_files" />
<link rel="statica/data" href="content/vehicles.csv" id="vehicles" />
<link rel="statica/data" href="content/notes.txt" id="notes" type="text/plain" />
```

Sources can be JSON, JSONL/NDJSON, CSV, plain text, Markdown, or globs. Data `href` must point to a file or explicit glob, not a directory. Data is loaded at build time. Output is static HTML.

Data `href` is a normal dynamic attribute: `${...}` placeholders are expanded before the source is loaded or globbed. Placeholders must be dotted paths that exist in the link's own scope. On pages, canonical roots such as `i18n` must be declared on `<html data-bind>`. In fragments, a data link can use the fragment template's bound values only when the link is inside that `<template>`.

```html
<!-- [locale]/index.html -->
<html data-bind="{i18n}">
  <head>
    <link rel="statica/data" href="../content/posts.${i18n.locale}.json" id="posts" />
  </head>
</html>
```

The data type is inferred from the file extension. You can also declare it with `type`:

```html
<link rel="statica/data" href="content/notes" id="notes" type="text/plain" />
```

Format shapes:

| File | Value |
| ---- | ----- |
| `.json` | Parsed JSON value |
| `.jsonl`, `.ndjson` | Array of one JSON value per non-empty line |
| `.csv` | Array of objects from the header row |
| `.txt`, `.text` | Array of non-empty strings, one per line |
| `.md`, `.markdown` | Object with frontmatter fields, `slug`, and `html` |

## Binding

statica source is valid HTML. It uses normal `<template>`, `<slot>`, and `<link>` elements as a build-time authoring flow, not as runtime Web Components. Keep those elements where HTML allows them; for example, do not put `<slot>` inside `<title>` or attributes.

Use `data-t` for scalar text content.

```html
<html data-bind="{item}">
  <h1 data-t="${item.headline}">Fallback heading</h1>
</html>
```

Use `${...}` only in attributes.

```html
<html data-bind="{item}">
  <a href="/posts/${item.slug}/" data-t="${item.headline}">Fallback link</a>
</html>
```

Every `${...}` placeholder in `data-t` and attributes must be a dotted identifier path available in the current binding scope. For pages, that scope is declared on `<html data-bind>` plus data link ids. For fragments, it is declared on `<template data-bind>` plus fragment-local data link ids. Literal `data-t` text renders as written. statica does not evaluate JS expressions such as `${a + b}`, and there is no magic flattening.

This rule applies to every attribute, including `href`, `src`, `class`, metadata attributes, and funnel `href` values. `data-t` is the special text-binding attribute; it replaces element text rather than emitting a literal attribute.

```html
<!-- direct fields -->
<template id="card" data-bind="{slug, headline}">
  <a href="/posts/${slug}/" data-t="${headline}">Fallback link</a>
</template>

<!-- whole object -->
<template id="card" data-bind="post">
  <a href="/posts/${post.slug}/" data-t="${post.headline}">Fallback link</a>
</template>
```

## Pages

Page binding uses a canonical context. The build process creates this object:

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

statica fills that object during the build:

| Root | Meaning |
| ---- | ------- |
| `data` | Linked data sources by `id`, such as `data.posts` |
| `item` | Current collection item for `[slug]` routes |
| `page.route` | Filesystem route, such as `posts/[slug]` |
| `page.params` | Route params resolved for the emitted page |
| `page.pagination` | Pagination metadata and page chunk for `[page]` routes |
| `i18n.locale` | Active locale, or empty when i18n is not active |

`<html data-bind>` declares which canonical roots the page uses. A page can use declared data link ids directly by `id`, but it must bind canonical roots such as `data`, `item`, `page`, or `i18n` before using them.

Page lookup order is: bound page data, declared data link ids, then no fallback. Data link ids cannot be `data`, `item`, `page`, or `i18n`.

### Static

No route params. One input page writes one output page.

```text
about/index.html -> .website/about/index.html
```

### 404 Pages

Missing-page output has two layers:

- Author-defined 404: add `404/index.html` or `404.html` to the site. It is built like any other static page, so it can use fragments, data links, styles, assets, and minification.
- Default 404: if neither `404/index.html` nor `404.html` exists in the built output, statica writes `.website/404/index.html` with a small default page.

The generated default 404 is a fallback artifact. It is not counted as an authored page in the build report and is not added to sitemap/RSS outputs. A custom 404 page is an authored page and takes precedence over the default.

`statica serve` and `statica watch` serve directory indexes normally. When a request misses every file, the preview server serves `404.html` first, then `404/index.html`, and returns HTTP status `404`. If an old output directory has no 404 page at all, the server still returns status `404`.

### Collection

Use a bracket folder with linked array-like data. Each emitted page receives one record as `item`.

```text
posts/[slug]/index.html
```

```html
<html lang="en" data-bind="{item}">
  <head>
    <link rel="statica/data" href="../../content/posts/*.md" id="posts" />
    <title>Post</title>
  </head>
  <body>
    <h1 data-t="${item.headline}">Post</h1>
    <div data-t="${item.html}"></div>
  </body>
</html>
```

The route param, here `slug`, is read from each item.

You can keep page usage direct by binding only the canonical roots the page needs:

```html
<html lang="en" data-bind="{item}">
  <a href="/posts/${item.slug}/" data-t="${item.headline}">Post</a>
</html>
```

### Pagination

Enable pagination with `[[pagination]]`, then place `[page]` folders where pages should be generated.

```text
blog/[page]/index.html
```

```html
<html lang="en" data-bind="{page}">
  <head>
    <link rel="statica/data" href="../../content/posts/*.md" id="posts" />
    <link rel="statica/fragment" type="text/html" href="../../ui/post-card.html" id="post-card" />
  </head>
  <body>
    <p>
      Page <span data-t="${page.pagination.page}"></span> of
      <span data-t="${page.pagination.total_pages}"></span>
    </p>
    <slot id="post-card" data-each="page.pagination.items"></slot>
    <a href="${page.pagination.prev_href}">Previous</a>
    <a href="${page.pagination.next_href}">Next</a>
  </body>
</html>
```

```toml
[[pagination]]
route = ""
page_size = 10
limit = 0
offset = 0
sort_by = "published_at"
sort_desc = true
max_pages = 0
index = true
```

`route` scopes the pagination rule to one route root such as `blog`; an empty route applies to every `[page]` route. `limit` caps the source items after sorting, `offset` skips items before paging, `max_pages` caps emitted page folders, and `index = true` also writes page 1 at the parent path.

`page.pagination` includes `items`, `page`, `page_number`, `total_pages`, `total_items`, `source_total`, `per_page`, `limit`, `offset`, `has_prev`, `has_next`, `prev`, `next`, `path`, `href`, `prev_href`, `next_href`, `first_href`, `last_href`, and `pages`.

Any route that includes `[page]` participates in pagination:

```text
blog/[page]/index.html         -> /blog/1/
blog/[page]/[slug]/index.html  -> /blog/1/my-post/
```

The listing page binds `{page}` and reads `page.pagination.items`. The nested item page binds `{page, item}`; `item` is taken from that page chunk, and `page.pagination` still points at the listing page metadata.

Do not put `[page]` and another collection route under the same route tree, such as both `posts/[page]` and `posts/[slug]`.

## Fragments

Fragments have three matching parts: import, mount, template.

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
    <a href="/posts/${slug}/" data-t="${headline}">Post</a>
  </article>
</template>
```

Loop with `data-each`.

```html
<template id="post-list" data-bind="{items}">
  <ul>
    <slot id="post-card" data-each="items"></slot>
  </ul>
</template>
```

Forward a mount `class` to a specific fragment element by ending one class token
with `+`.

```html
<!-- page -->
<slot id="button" class="hero-cta"></slot>
```

```html
<!-- ui/button.html -->
<template id="button" data-bind="{label}">
  <a class="btn+" href="/start/" data-t="${label}">Start</a>
</template>
```

The emitted class is `class="btn hero-cta"`. A bare `class="+"` forwards the
mount class without keeping a base class. Only `class` forwarding is supported;
other mount attributes stay on the mount and are not forwarded.

Project mount children into fragment slots. Children without a `slot` attribute
go to the unnamed default slot. Children with `slot="name"` go to
`<slot name="name">`.

```html
<!-- page -->
<slot id="card">
  <h2 slot="header">Projected title</h2>
  <p>Projected body</p>
</slot>
```

```html
<!-- ui/card.html -->
<template id="card">
  <article class="card">
    <header>
      <slot name="header">Fallback title</slot>
    </header>
    <slot><p>Fallback content</p></slot>
  </article>
</template>
```

When projected children are present for a slot, they replace that slot. When no
children are passed for a slot, the fallback content inside the fragment slot is
kept. Each fragment template may declare one default slot and one slot for each
name; duplicate projection slots fail the build.

Fragment paths are relative to the file that declares them. Fragments may import their own data and other fragments.

Fragments do not inherit canonical page roots. A fragment can read only values passed through its render context and names introduced by its own data links. Use `data-each` on mount slots for loops; keep `data-bind` on the fragment `<template>`.

## Layouts

Layouts are build-time document shells for reusable page structure. A page declares one layout with `<link rel="statica/layout" href="...">`. statica loads the layout, projects page content into layout slots, and then runs the normal data, fragment, binding, scoping, forms, assets, manifest, search, and minify flow on the merged document.

```html
<!-- layouts/base.html -->
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <slot name="head"></slot>
  </head>
  <body>
    <header><slot name="nav"><a href="/">Home</a></slot></header>
    <main><slot></slot></main>
    <aside><slot name="sidebar">Fallback sidebar</slot></aside>
  </body>
</html>
```

```html
<!-- index.html -->
<!doctype html>
<html lang="en">
  <head>
    <link rel="statica/layout" href="layouts/base.html" />
    <title>Home</title>
    <link rel="statica/data" href="content/site.json" id="site" />
  </head>
  <body>
    <nav slot="nav"><a href="/blog/">Blog</a></nav>
    <h1>Hello layout</h1>
    <template slot="sidebar"><p>Projected side</p></template>
  </body>
</html>
```

Projection rules:

- Page `<head>` children except the layout link project into `<slot name="head">`.
- Page body children without `slot` project into the default `<slot>`.
- Page body elements with `slot="name"` project into `<slot name="name">`; statica removes the `slot` attribute from the projected element.
- `<template slot="name">` projects its children into the named slot without keeping the `<template>` wrapper.
- If no content is projected for a slot, the fallback children inside the layout slot are kept.

Layout files may include their own statica data and fragment links. Relative `statica/data` and `statica/fragment` hrefs inside a layout resolve from the layout file before the merged page is prepared, so shared layout fragments can live beside the layout.

```html
<!-- layouts/base.html -->
<html lang="en">
  <head>
    <link rel="statica/fragment" type="text/html" href="./ui/banner.html" id="banner" />
    <slot name="head"></slot>
  </head>
  <body>
    <slot id="banner"></slot>
    <main><slot></slot></main>
  </body>
</html>
```

Pages may still bind canonical roots such as `{data}`, `{item}`, `{page}`, and `{i18n}` on their `<html>` element. If the layout `<html>` has no `data-bind`, statica carries the page `<html data-bind>` onto the merged layout document. If the layout declares its own `data-bind`, that layout contract wins. Binding validation uses the merged document.

## CSS And JS

Inline `<style>` in pages and fragments is always transformed with lightningcss.

```html
<style>
  .card {
    padding: 1rem;
    & h2 { font-weight: 700; }
  }

  @media (width >= 40rem) {
    .card { padding: 1.5rem; }
  }
</style>
```

Fragment CSS is scoped with a generated `data-s` attribute.

Fragment scripts are scoped to the fragment instance by default. Selector methods
on `document` only search inside the current fragment instance.

```html
<template id="button">
  <button class="btn"><slot>Go</slot></button>
  <script type="module">
    document.querySelector(".btn")?.addEventListener("click", () => {
      document.querySelector(".btn").dataset.pressed = "true";
    });
  </script>
</template>
```

Linked `.css` under `asset_dirs` is transformed when `[process].css` is enabled. Final minification is controlled by `[minify]`.

## Fonts

Use `statica/font` for stylesheet-like font links.

```html
<link rel="statica/font" href="@Google/?family=Outfit:wght@100..900&display=swap" />
<link rel="statica/font" href="./fonts/outfit.css" />
```

statica includes a Google Fonts recipe: when a resolved `statica/font` stylesheet URL points at `fonts.googleapis.com`, it emits the Google preconnect hints once per page. Other local or remote font CSS emits a normal stylesheet link. Font files are copied through `asset_dirs`.

## Forms

Mark static forms with `statica`.

```html
<form name="contact" statica>
  <input type="email" name="email" required />
  <button type="submit">Send</button>
</form>
```

Configure the endpoint in `statica.toml`.

```toml
[forms]
enabled = true
provider = "formspree"
endpoint = "https://formspree.io/f/{id}"

[forms.ids]
contact = "xyzabc"
```

statica writes `action` and `method="POST"`. It does not inject client JavaScript.

Build-time env vars can provide form endpoints and ids without hard-coding them in committed config. statica loads inline `[env]` values, then `.env` and `.dev.vars` from the config directory when `[env].load_files` is true; existing process env vars win.

```toml
[env]
load_files = true
FORMS_CONTACT_ID = "xyzabc"
```

## Search

Add a generated search modal with one authoring input.

```html
<input type="statica/search" placeholder="Search" />
```

During build, statica replaces that input with an accessible rounded icon
button and `<dialog>` search modal, emits `/search.json`, and writes the small
runtime files at `/statica/search.js` and `/statica/search.css`. Search runs in
the browser against the generated JSON index.

Search behavior is configured at build time:

```toml
[search]
enabled = true
output = "search.json"
limit = 10
filters = ["tags", "categories"]
url_field = "url"
```

`output` defaults to `search.json`. `limit` defaults to `10`. `filters`
declares result filters such as `tags,categories` or
`make,status,condition,location`; the generated modal renders them as clickable
filter controls. `url_field` sets which search result field is used for result
links and defaults to `url`.

To emit an index for a custom search UI without using the generated modal:

```toml
[search]
enabled = true
output = "search.json"
```

The generated index is an array of page records:

```json
[
  {
    "url": "/posts/hello/",
    "title": "Hello",
    "section": "posts",
    "text": "Plain searchable page text",
    "excerpt": "Plain searchable page text",
    "meta": [
      { "name": "description", "value": "A short page summary" }
    ]
  }
]
```

statica indexes emitted HTML pages, excluding script, style, template,
noscript, dialog content, and the generated 404 page.

## Sitemap And RSS

Sitemap and RSS output need `site_url` so statica can write absolute URLs for the deployed site.

```toml
site_url = "https://example.com"

[sitemap]
enabled = true
filename = "sitemap.xml"
urls_per_file = 50000

[rss]
enabled = true
filename = "rss.xml"
title = "Blog"
description = "Latest posts"
language = "en"
limit = 50
title_field = "headline"
description_field = "summary"
date_field = "published_at"
collections = ["posts"]
```

`[sitemap].urls_per_file` splits large sites into numbered sitemap files plus an index. RSS reads collection records and maps fields with `title_field`, `description_field`, and `date_field`; an empty `collections` list includes every collection.

## i18n

Use `[locale]` in the route and enable `[i18n]`.

```text
[locale]/about/index.html -> .website/en/about/, .website/pt/about/, ...
```

```toml
[i18n]
enabled = true
default = "en"
locales = ["en", "pt"]
dir = "content/i18n"
fallback = ""
```

Catalogs live at `content/i18n/{locale}.json`. `fallback` names the catalog used for missing keys; an empty fallback uses the default locale.

```html
<html data-bind="{i18n}">
<span data-t="${i18n.nav.home}">Home</span>
<a href="/${i18n.locale}/">Home</a>
</html>
```

i18n catalog values live under `i18n.*`, and pages must bind `{i18n}` before using them. The same binding rules apply here: `${...}` works in attributes and `data-t`, not text nodes, and placeholders must be dotted identifier paths. Fragments do not receive `i18n` automatically; pass translated values into the fragment or link fragment-local data.

Locale-specific data can use canonical `i18n.locale` in funnel `href`.

```html
<link rel="statica/data" href="../../../content/posts.${i18n.locale}.json" id="posts" />
```

statica expands dynamic attribute placeholders before loading the data source, so locale-specific globs work too:

```html
<link rel="statica/data" href="../../../content/posts.${i18n.locale}/*.md" id="posts" />
```

## Aliases

Aliases allow short prefixes instead of repeating long local paths or URLs. They live in `statica.toml`, and the default leading symbol is `@`.

```toml
[aliases]
symbol = "@"

[aliases.urls]
Google = "https://fonts.googleapis.com/css2"

[aliases.paths]
static = "./static"
fonts = "./assets/fonts"
ui = "./ui"
```

Use aliases anywhere statica resolves authoring paths, such as fonts, scripts, styles, fragments, data funnels, and assets.

```html
<link rel="statica/font" href="@Google/?family=Outfit&display=swap" />
<script type="module" src="@static/app.js"></script>
<link rel="statica/fragment" href="@ui/post-card.html" id="post-card" />
```

`[aliases.urls]` entries resolve to absolute URLs. `[aliases.paths]` entries resolve to local paths relative to `statica.toml`. The text after the alias name is preserved as the tail, so `@static/app.js` resolves against the `static` alias base.

## Assets

| Asset kind | Tool |
| ---------- | ---- |
| CSS | lightningcss |
| JS | oxc |
| HTML | minify-html |
| Images | oxipng + image |
| Fonts | copied as-is |

```toml
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
```

`[process]` handles copied assets. `[minify]` runs a final pass over emitted files in `out_dir`.

When `[process].enabled` and `[process].images` are on, statica optimizes copied raster images, writes responsive width variants, adds configured formats such as WebP, and rewrites local `<img>` tags to responsive `<picture>` markup when `[process.image].responsive` is true. Use `[process.image]` to control target widths, extra formats, JPEG quality, and the default `sizes` value.

```html
<img src="/images/hero.jpg" alt="Hero image" sizes="(width >= 60rem) 50vw, 100vw" />
```

The original image width is always included when smaller than a configured target width. Responsive processing applies to local raster image assets copied through `asset_dirs`; SVGs and remote images are left as authored.

## Web Manifest

Enable with `manifest = true` or `--manifest`.

```toml
manifest = true
```

statica scaffolds `public/manifest.webmanifest` if missing, copies it, and injects manifest/theme/apple-touch tags unless the page already has them.

## Config

`statica.toml` is optional. Missing file means defaults.

```toml
project = ""
out_dir = ".website"
clean = true
copy_assets = true
asset_dirs = ["public", "assets", "static"]
ignore_dirs = [".website", "dist", "target", ".git"]
site_url = ""
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

[preview]
host = "0.0.0.0"
port = 4321
debounce_ms = 80
poll_interval_secs = 2

[sitemap]
enabled = false
filename = "sitemap.xml"
urls_per_file = 50000

[rss]
enabled = false
filename = "rss.xml"
title = ""
description = ""
language = "en"
limit = 50
title_field = "headline"
description_field = "summary"
date_field = "published_at"
collections = []

[search]
enabled = false
output = "search.json"
limit = 10
filters = []
url_field = "url"

# [[pagination]]
# route = ""
# page_size = 10
# limit = 0
# offset = 0
# sort_by = "published_at"
# sort_desc = true
# max_pages = 0
# index = true

[forms]
enabled = false
provider = "formspree"
endpoint = "https://formspree.io/f/{id}"
endpoint_env = "FORMS_ENDPOINT"

# [forms.ids]
# contact = "your-form-id"

[i18n]
enabled = false
default = "en"
locales = ["en"]
dir = "content/i18n"
fallback = ""

[env]
load_files = true
# FORMS_CONTACT_ID = "your-form-id"

[performance]
render_mode = "auto"
render_threads = 0
```

CLI SPEC flags override TOML.

```bash
statica build --process 'css=true,js=false,images=true'
statica build --minify 'html=true,css=true,js=true'
statica build --search 'output=assets/search.json'
statica build --pagination 'page_size=10,index=true'
statica build --i18n 'locales=en|pt,default=en'
statica build --render-mode serial
statica build --render-mode parallel --render-threads 8
statica build --report-json report.json
statica watch --preview host=127.0.0.1,port=9000
```

`[performance].render_mode` controls page rendering only:

- `auto` uses statica's default page-render profile.
- `serial` avoids rayon for page rendering, useful for cold-start or profiling comparisons.
- `parallel` always uses rayon for route emission and high-cardinality collection/paginated item pages.

`render_threads = 0` uses the default rayon worker count when rendering in parallel; any positive value caps the parallel page-render pool. Output lists stay deterministic.

`--report-json [PATH]` writes the build report as JSON. It includes counts, warnings, outputs, route rows, phase timings, and total duration. Omit `PATH` or pass `-` to write to stdout; pass a file path for CI artifacts or benchmark logs. In `watch`, statica writes the report after the initial build and every rebuild.

`statica watch` rebuilds conservatively. Direct edits to an existing page `index.html` re-emit only that page route when `[process]` and `[minify]` are disabled. Edits to shared inputs such as linked data, fragments, assets, config-driven processing, deleted files, or minified builds use a full rebuild so generated output stays consistent.

## Deploy

`statica build` writes plain static files to `.website/` by default. Deploy that output directory to any static host.

```bash
statica build
```

Set `site_url` when enabling sitemap or RSS output so generated absolute URLs match the deployed origin.
