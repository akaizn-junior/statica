# statica guide

**Just HTML.** This guide is the direct reference for authoring rules and configuration.

For install and starting a new site, use [../README.md](../README.md).

statica source files are valid HTML. Data, fragments, translations, forms, and pagination are resolved at build time; the generated site is plain static HTML/CSS/JS.

## Commands

```bash
statica [PATH]              # build (default)
statica build [PATH]        # build (explicit)
statica watch [PATH]        # watch + rebuild + serve
statica serve [PATH]        # serve out_dir
statica new <NAME>          # scaffold
```

`PATH` defaults to `.`. statica resolves it against the process cwd, walks up for `statica.toml`, then uses that directory as the site root. If `project` or `--project` is set, the site root is that subdirectory under the config directory.

## Project Layout

```text
my-site/
├── statica.toml
├── index.html
├── content/
│   ├── posts.json
│   └── i18n/en.json
├── ui/
│   └── post-card.html
├── posts/[slug]/index.html
├── blog/[page]/index.html
└── public/
```

Every `index.html` is a page. Folder path is the route.

```text
index.html                 -> .dist/index.html
about/index.html           -> .dist/about/index.html
posts/[slug]/index.html    -> .dist/posts/{slug}/index.html
blog/[page]/index.html     -> .dist/blog/1/, .dist/blog/2/, ...
```

## Data

Load build-time data with `statica/data`.

```html
<link rel="statica/data" href="../content/posts.json" id="posts" />
```

Sources can be JSON, JSONL/NDJSON, CSV, plain text, Markdown, or globs. Data `href` must point to a file or explicit glob, not a directory. Data is loaded at build time. Output is static HTML.

The data type is inferred from the file extension. You can also declare it with `type`:

```html
<link rel="statica/data" href="../content/notes" id="notes" type="text/plain" />
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
<h1 data-t="${headline}">Fallback heading</h1>
```

Use `${...}` only in attributes.

```html
<a href="/posts/${slug}/" data-t="${headline}">Fallback link</a>
```

Every `${...}` placeholder in `data-t` and attributes must be a dotted identifier path available in the current binding scope. For pages, that scope is declared on `<html data-bind>` plus data link ids. For fragments, it is declared on `<template data-bind>` plus fragment-local data link ids. Literal `data-t` text renders as written. statica does not evaluate JS expressions such as `${a + b}`, and there is no magic flattening.

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
about/index.html -> .dist/about/index.html
```

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
    <slot name="item.html"></slot>
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
  <body>
    <p>
      Page <span data-t="${page.pagination.page}"></span> of
      <span data-t="${page.pagination.total_pages}"></span>
    </p>
    <slot id="post-list"></slot>
    <a href="${page.pagination.prev_href}">Previous</a>
    <a href="${page.pagination.next_href}">Next</a>
  </body>
</html>
```

```toml
[[pagination]]
page_size = 10
sort_by = "published_at"
sort_desc = true
index = true
```

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
<link rel="statica/fragment" type="text/html" href="../ui/post-card.html" id="post-card" />
<slot id="post-card"></slot>
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

Fragment paths are relative to the file that declares them. Fragments may import their own data and other fragments.

Fragments do not inherit canonical page roots. A fragment can read only values passed through its render context and names introduced by its own data links. Use `data-each` on mount slots for loops; keep `data-bind` on the fragment `<template>`.

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

Fragment scripts use `$` for the fragment instance.

```html
<template id="button">
  <button class="btn"><slot>Go</slot></button>
  <script type="module">
    $.querySelector(".btn")?.addEventListener("click", () => {
      $.host.dataset.pressed = "true";
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

Google Fonts links emit preconnect hints. Local font CSS emits a normal stylesheet link. Font files are copied through `asset_dirs`.

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

## i18n

Use `[locale]` in the route and enable `[i18n]`.

```text
[locale]/about/index.html -> .dist/en/about/, .dist/pt/about/, ...
```

```toml
[i18n]
enabled = true
default = "en"
locales = ["en", "pt"]
dir = "content/i18n"
```

Catalogs live at `content/i18n/{locale}.json`.

```html
<html data-bind="{i18n}">
<span data-t="${i18n.nav.home}">Home</span>
<a href="/${i18n.locale}/">Home</a>
</html>
```

i18n catalog values live under `i18n.*`, and pages must bind `{i18n}` before using them. The same binding rules apply here: `${...}` works in attributes and `data-t`, not text nodes, and placeholders must be dotted identifier paths. Fragments do not receive `i18n` automatically; pass translated values into the fragment or link fragment-local data.

Locale-specific data can use `${locale}` in funnel `href`.

```html
<link rel="statica/data" href="../../../content/posts.${locale}.json" id="posts" />
```

## Aliases

Aliases live in `statica.toml`.

```toml
[aliases]
symbol = "@"

[aliases.urls]
Google = "https://fonts.googleapis.com/css2"

[aliases.paths]
static = "./static"
fonts = "./assets/fonts"
```

Use them as regular paths.

```html
<link rel="statica/font" href="@Google/?family=Outfit&display=swap" />
<script type="module" src="@static/app.js"></script>
```

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

[minify]
enabled = false
html = true
css = true
js = true
```

`[process]` handles copied assets. `[minify]` runs a final pass over emitted files in `out_dir`.

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
out_dir = ".dist"
clean = true
copy_assets = true
asset_dirs = ["public", "assets", "static"]
ignore_dirs = [".dist", "dist", "target", ".git"]
site_url = ""

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
```

CLI SPEC flags override TOML.

```bash
statica --process 'css=true,js=false,images=true'
statica --minify 'html=true,css=true,js=true'
statica --pagination 'page_size=10,index=true'
statica --i18n 'locales=en|pt,default=en'
statica watch --preview host=127.0.0.1,port=9000
```
